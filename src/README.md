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

`hd check` skips the test cases and test-only functions of a `tests:` block
unless `--tests` is given (Testing T42); `hd test` always compiles them.
`hd test` (`test-runner.ts`) runs each test case in a fresh instance of one
compilation. An `it_each` table is one test function that the runner calls
once per row, as `name[i]`, through the exported `__hd_each_index` and
`__hd_each_count` globals; a failure names the test case. A test function
with a `timeout` evaluates it first and reports its milliseconds through
`__hd_timeout_ms`; the runner fails a test case whose call took longer.
It checks after the call returns, so it cannot stop a body that never
returns.

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
worker ([`../website/playground/README.md`](../website/playground/README.md)).
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
[the package linker](../website/playground/README.md#packages-and-modules); it
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
- the sized numeric types `i8` to `i64`, `u8` to `u64`, `f32`, and `f64`,
  `bool`, Unicode-scalar `char`, and UTF-8 `string` values
  (`src/numeric.ts`); an integer or float literal takes its type from the
  expected type and is range-checked, integers widen implicitly within one
  signedness family and `f32` widens to `f64`, a narrowing is
  `implicit-narrowing`, and mixing the families is `mixed-signedness`.
  Every integer of at most 32 bits is an `i32` at run time and `u64` an
  `i64` read as unsigned; arithmetic on the narrow types range-checks its
  result (`emitter/sized-numeric.ts`). Constructor-style casts such as
  `i16(wide)` wrap to the target width, a literal argument is range-checked
  against the target, and a float-to-integer cast saturates through the
  non-trapping `trunc_sat` instructions, clamping a narrow target in `f64`
  first (`types.cast.saturate`);
- heterogeneous tuple literals, tuple types, simultaneous tuple destructuring,
  and statically typed `._0` selection, stored in erased Wasm GC arrays;
- checked integer arithmetic and exponentiation at every width, IEEE `f32` and `f64` power, UTF-8 string
  concatenation, scalar and string comparisons, Wasm GC reference identity,
  boolean short-circuiting, and explicit panics;
- interpreted `$name` and `${expression}` string segments with left-to-right
  canonical `Display` dispatch for concrete implementations, generic bounds,
  dynamic trait values, and the standard `string`, numeric, `bool`, and
  `char` implementations; an `f32` shows its own shortest round-trip digits
  through the host's `format_f32`;
- `println` with the same display surface, statically requiring a
  lexical `Console` provider. `Console` is a prelude trait with
  `write_line!(mut self, text: string) -> Result[void, ConsoleError]`, so
  `$.use(Console)` is `mut Console` and a program may implement it. The
  host console is a `Console` trait value that boxes the host's `externref`
  and goes through the generic host capability bridge
  (`emitter/host-providers.ts`); its `write_line!` writes the line and is
  ready with `.Ok()` on its first poll, so direct calls run on it and on a
  program-defined provider. `println` calls `write_line!` on the covering
  provider, the host console or a program-defined one, and drives the call
  with `block_on` (MHP-1), so it panics with `suspension-nested-driver`
  under `main!` or a test body and is `suspension-forbidden-context` in a
  `defer` suite or a default expression. A call that returns `.Err` is an
  `explicit-panic` from `std`'s `panic`. A write pending on a host
  operation is polled again until it finishes, as the entry driver polls
  `main!`, since the prototype's host answers each poll itself. A script's
  top-level `println` is rejected with `missing-requirement`, because the
  prototype infers no script entry requirement row
  (`module.init.script-row`). A public non-suspending function with a
  host provider in its row is exported through a wrapper that makes the
  trait value from the host's `externref`;
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
  requirement-free ordered defaults, per-variant factories, named or `._0` shared-field access, and
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
  provider overrides that escape their `$.with` scope; a captured `let` is a
  shared heap cell, and every closure may assign it and keep mutable
  captures (`mut fn` is a syntax error); a generic function used as a value is
  instantiated from explicit type arguments or the expected function type;
  function types convert by declared variance (permission changes only),
  are implementation targets owned by the standard library, reject a direct
  `is`, and may be spelled `Fn[...]`, `SuspendFn[...]`, and `Rest[T]` when
  imported from `std.function`;
- a data, enum, trait, primitive, or type parameter name where a value is
  required is `type-used-as-value`; a type alias name there still reports
  `unknown-name`, because aliases are expanded before checking;
- `type` aliases, generic ones included, expanded before checking, with
  `alias-cycle` for a cycle; row aliases (`type AppRow = Db + Cache`,
  generic and nested) expanded in every row and bare in a one-key row slot
  (`$.Context[AppRow]`, `Fn[(), void, AppRow]`, an explicit row type
  argument), and `generic-kind-mismatch` for one used as a type or single
  key; newtypes lowered to one-field
  data types; `data`, `enum`, `trait`, `type`, and `impl` in a block suite,
  hoisted under a scoped name; the prelude traits `Any` and `Iterable` (user
  implementations and bounds drive `for` loops and comprehensions, and
  collections satisfy `Iterable` bounds); declared `+T`/`-T` variance with
  readonly variance conversions; row type arguments such as
  `Fn[(), void, $ Logger + Clock]`, with `generic-kind-mismatch` for a data,
  enum, or trait parameter used in a row; a dynamic trait value satisfying
  bounds on its own trait and supertraits through forwarding dictionaries;
- concrete requirement rows with hidden `externref` provider threading and
  transitive call paths from `hd explain-requirements`;
- `+`-joined requirement rows (`$ A + B` in every position) normalized as
  sets, with `old-row-separator` for the former `$ A, B` and `$(A, B)` and
  `old-bound-operator` for a `+` between bounds, plus statically resolved `$.use`
  (including ordered multi-provider tuple lookup) and lexical `$.with`
  provider overrides;
- requirement-bearing closure types with invocation-time provider threading and
  least-row inference for requirements not satisfied by lexical providers;
- generic requirement-row parameters with least-row inference, symbolic and
  concrete row union, repeated-row consistency, removal of a key by row
  extension (`$ R + K` in the callback row, `$ R` on the callee), keyed Wasm
  GC provider packs, and lexical restoration of removed providers;
  `ambiguous-row-pattern` for a pattern with two unfixed row parameters and
  `row-parameter-in-context` for `$.Context[R]`; explicit row type arguments
  for a function's row parameters, as in `provide[$ Db + Log](job)`;
- row subsumption: a function value with a narrower concrete row fits a
  wider function type through the callable adapter. A value is not widened
  into a row that holds a row parameter it lacks, which least-row inference
  solves instead. A list or map literal with no expected type gives its
  function values the union of their rows; a literal with a spread does
  not yet (`KNOWN_FAILURES.tsv`, RU12), and diagnostics print the union's
  expanded keys rather than the rows as written. `if` branches, `match`
  arms, and inferred closure and function results take the union too
  (RU15), and a branch or arm type mismatch is `no-common-type`. A
  function whose result is a function type with a row fails Wasm
  validation, since its type string reads the inner row as the outer
  function's (`KNOWN_FAILURES.tsv`, RU15);
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
  duplicate-member diagnostics; `impl[T] Box[T]:` and `impl Box[i32]:` targets
  lower to erased generic functions whose target parameters come from the
  receiver, and one name clashes only when two targets unify;
- receiverless associated functions called through `Type::function`, including
  `Self` substitution, method-level generics, and suspending calls, inherent
  first and then the implemented traits (`ambiguous-method` for two);
  `T::function()` on a type parameter calls through the bound's dictionary;
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
  entries, and explicit override precedence; a default body calls through
  `self` only methods of its trait and supertraits (`unknown-method`);
- bound proofs deeper than 64 nested implementation bounds are
  `trait-resolution-depth`, including bounds of method-less marker
  implementations;
- generic supertraits substitute parent arguments through inherited calls and
  checked trait-value widening; child dictionaries retain blanket or concrete
  parent implementations;
- suspending trait methods with typed dynamic GC-frame wrappers, trait-bound
  dispatch, stored driving, and cancellation forwarding;
- erased generic parameters with independent GC dictionaries for multiple
  trait bounds, dictionary forwarding, method dispatch on values produced
  inside generic bodies, and concrete call-site recovery for returned `T`
  values;
- concrete and bounded generic `Eq` and `PartialOrd` dispatch, with
  structural equality for tuples, lists, optionals, `Result`, and maps and
  lexicographic tuple/list plus `.None`-first optional ordering, recursively using
  explicit implementations and erased bound dictionaries for nested values;
  primitives and those built-in composites also satisfy `Eq` and
  `PartialOrd` bounds (floats included, with IEEE equality), and the ones
  without floats satisfy `Ord`; `PartialOrd < Eq` and `Ord < PartialOrd`, so
  each generated dictionary carries its supertrait's; primitives satisfy
  `Display` bounds and become `Display` trait values, through generated
  standard-library dictionaries;
- generic data declarations with inferred or complete explicit construction
  arguments, precise instantiated member types, and uniform `anyref` field
  erasure in one Wasm GC layout per declaration;
- generic enums with inferred and contextual construction, recursive
  instantiations, precise pattern bindings, and uniform `anyref` payload
  erasure in one Wasm GC layout per declaration;
- trait values as lexical providers, including dispatch after generic
  requirement-row packing and removal by extension;
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
  initialization order. A `*_test.hd` test module joins as a `tests:`
  block, and a test build links every test module; `hd test FILE` parses a
  `*_test.hd` file as a test module, whose top level is test position. Files
  of one folder may use each other in a loop; a loop of folders is
  `folder-cycle`, reported once per tangle with one shortest folder loop,
  each edge's `use` line, the tangle size, and an `x.hd` to `x/mod.hd`
  fix-it; uses in test code make no folder edge
  (10-modules.md#dependency-cycles). Modules that use each other form one
  initialization group, joined by module identity after the groups it
  uses. The prototype does not order a group's statements by dependency
  (10-modules.md#order-inside-a-group), so a read that needs a later-joined
  module's binding is `top-level-read-before-initialization`. Linked
  modules share one top-level namespace, and namespace or renaming uses of
  package declarations are not supported
  (`../website/playground/README.md#packages-and-modules`);
- imported `std.resource.ResourceError[E]` as the canonical generic
  `Operation(E) | Disposed` enum, using the same erased Wasm GC representation
  as source-declared generic enums;
- literal suffixes (Literal Suffixes L1-L18, L11 with Decorators
  D9's names): `250ms` and `1.5kb` lex as one number with a suffix, and the
  parser desugars them to the call `ms(250)`, folding a directly applied
  `-` into the literal. Radix literals take no suffix, and a reserved-word
  suffix such as `5else` is `invalid-token`. The checker resolves the
  suffix among module-scope functions only, requires a function marked
  `@num_suffix` (`FunctionDecl.numSuffix`, set after the std join by
  `checker/decorators.ts` from a value of `std.ops.NumSuffix`), and checks
  the ordinary call, so a generic or provider-needing suffix function
  follows the ordinary rules (L20). The marked function's shape, exactly
  one parameter of a primitive number type or of a type parameter bounded
  by `std.num.Num`, `Integer`, or `Float`, and no suspension, is checked at its definition as
  `type-mismatch` on the `fn` line (L21, L22, `checker/literal-suffixes.ts`); an
  unmarked function at the literal is `invalid-literal-suffix`. `std.ops` and
  `std.time` (`Duration`, an `i64` count of milliseconds in the
  prototype's own `millis` field, and the suffix functions `ms`, `s`,
  `min`, and `h`) come from the [standard library](#standard-library), so
  a library suffix function works. A test `timeout` is checked as a `Duration` and enforced after the
  body returns (see `hd test` above);
- string prefixes (Literal Suffixes L19): an identifier directly before
  `"` lexes with the string as one token (`Token.prefix`) whose text is raw
  but still interpolates. The parser desugars `x"a $b c"` to the call
  `x(Template { raw_parts: ["a ", " c"], values: [b] })`, naming the
  template type by its hidden name `__std_ops_Template`, which
  `checker/standard-library.ts` renames when the program imports
  `Template`. The checker resolves the prefix among module-scope functions
  only, requires `@str_prefix` (`FunctionDecl.strPrefix`, set with the
  suffix marker in `checker/decorators.ts` from a value of
  `std.ops.StrPrefix`), and checks the ordinary call, so each value
  converts to the template's `T` like an argument. The prefix shape,
  exactly one parameter, of type `Template[T]`, and no suspension, is
  checked at the definition as `type-mismatch` on the `fn` line
  (`checker/literal-suffixes.ts`); an unmarked function at the string is
  `invalid-string-prefix`. `std.text` declares `r`, `interpolate`, and
  `process_escapes` (which returns `Result[string, EscapeError]` with the scalar offset of
  the bad escape) in hd; a `\u{...}` escape uses the host function
  `string_from_scalar`;
- imported `std.convert.From[T]` and `std.error.Error` as trait
  declarations in the compiled module; `?` on a `Result` converts the error
  by one assignability rule or one `From` call, `Type::from(x)` selects the
  `From` instantiation by argument type, and a single-payload variant
  constructor is a function value (a dynamic trait value does not yet
  satisfy a bound on its own trait). A generic function or generic variant
  constructor passed as a call argument takes its type arguments from the
  call. While a result type is inferred, `?` converts nothing and its
  operand's error must match the inferred result;
- test bodies (Testing T4): a `test` block's result is inferred like a
  closure's and must be `void` or a `Result` with a `Display` error, else
  `unsatisfied-trait-bound`; a test fails when `report()` on its result
  gives a nonzero code, as for `.Err`. Importing `std.process.ExitCode` or
  `Termination` declares both from the
  [standard library](#standard-library) (Testing T8), with the implementations for
  `ExitCode`, `void` (whose `self` is a null `anyref`), and `Result[T, E]`;
  `main` and `main!` may return `void`, `ExitCode`, or a `Result` over them
  (a program's own `Termination` type is reported as not yet supported), and
  `hd run` exits with the code, reporting an `.Err` as `main returned Err`
  with code 1. Printing the error's `Display` text and cause chain is not
  implemented;
- typed derivation (spec/14-annotations.md#typed-derivation, Typed
  Derivation M1-M29), lowered before checking by `checker/typed-derivation.ts`:
  decorators on data, enum, newtype, field, variant, payload, and function
  parameter declarations; the `+=` token; `@derive` of a trait with a
  `by Structure` template; derivation blocks with member lines (`=`, `+=`,
  `= pass`, `Self`); trait-less derivation blocks `impl T by Structure:`
  (M26), which `checker/member-lines.ts` checks and folds into the
  declaration facts of `T` before any derivation reads them, then drops;
  and the `std.structure` handles, facts, walkers,
  describers, and sources, declared in hd when imported. Each derivation
  becomes an ordinary `impl` whose template bodies call generated `walk`,
  `describe`, `build`, and `facts` functions, specialized to the target and
  to the walker, describer, or source type; the template must hold that
  value in a local declared with its type (`unsupported-derivation`
  otherwise). A walker's `member` may strengthen its bound; its dictionary
  entry traps, since only generated code calls it, concretely. The checks
  of the chapter's diagnostics (`underivable-trait`, `misplaced-derivation`,
  `marker-template`, `invalid-member-line`, `duplicate-fact`,
  `omitted-member-without-default`, `member-not-derivable`,
  `generic-member-call`, `newtype-derivation-self`, `gadt-derivation`, the
  two warnings, the `structure-variant-mismatch` panic, and
  `suspension-forbidden-context` in facts) are implemented. Gaps: `Facts`
  holds `Inspectable` values rather than `Any`, so a fact must be
  inspectable; a fact's concrete type for `duplicate-fact` is read from
  syntax (a data literal or a call's declared result), for member lines and
  for declaration facts alike (M25); `VariantInfo.shared`
  is always empty; a build handle's `get` returns the declared type whatever
  its argument's permission; a newtype forwards only through the receiver
  and plain `Self`; `@derive(Eq)`, `PartialOrd`, `Ord`, and `Hash` are
  generated as ordinary hd implementations (`checker/derive-intrinsics.ts`),
  on a newtype through its base type, and `mixed-derived-law` checks the
  law partners; `==` uses a generic implementation such as a derived
  `impl[T < Eq] Eq for Box[T]`, while `<` does not yet. `@derive(Debug)` generates
  builder calls in Rust's mapping (Testing T53, T54): `debug_struct` for a
  data type, even a fieldless one, and a variant with named payload fields,
  `debug_tuple` for a variant with positional ones, and `write` of a
  payload-free variant's name. A variant that mixes both uses
  `debug_struct`, naming a positional field `_0`, `_1`, and so on. A field
  a derivation compares or hashes without the trait is
  `derive-field-missing-trait` at the field (at the base type for a newtype), and a use whose added bound
  fails is `missing-derived-bound`. `Debug` is a prelude trait; `DebugWriter`, its
  builders, the prelude `debug`, and `std`'s `Debug` implementations for
  the primitives, `List`, `T?`, `Result`, and pairs are hd code in
  `lib/std/format.hd`, whose writer is always compact. The checker still
  accepts `Debug` for `Map` and longer tuples, which render no text; the drift and unused-fact warnings treat
  the module as one package, and the unused-fact warning skips a literal
  fact such as `@"note"` and a fact built by a name imported from `std`,
  such as `@annotate(.Field)` (M25). `@derive` before a function, trait,
  implementation, or method is `decorator-not-annotator`. Trait-less
  blocks follow M27-M29: the header must bind the declaration's parameters
  in order, under any names and without bounds; a second block for one type
  is `overlapping-impl`; a `Self` line's fact warns as unused when the type
  derives nothing; and a member line's right side may be any list-typed
  expression. A name bound by a module `let` to a list literal is inlined,
  so its elements keep their concrete types; any other list expression is
  spread into `Facts` as `value...`, which needs `Inspectable` elements. A
  right side whose type is known from syntax and is not a list, such as
  `name = 5`, is `invalid-member-line`. Two type-level decorators of one
  fact type are `duplicate-fact` on the later one. The per-trait `Self`
  line warning needs a second package, which the prototype CLI cannot
  load. The prototype cannot check `annot.traitless.module` across the
  modules of a linked package, which share one namespace;
- decorators as plain values (spec/14-annotations.md#prefix-decorators,
  Decorators D1-D10), in `checker/decorators.ts`: a decorator before a
  function, trait, implementation, newtype, method, or method parameter
  attaches its value, which is checked as a compile-time expression like
  any fact, counted by `duplicate-fact`, and, on a module-level function,
  read by `shape_of(f).metadata[M]()`. A bare decorator name of a function
  with no parameters is rewritten to a call, before typed derivation for
  the program's own and imported functions and after the standard library
  is joined for `std`'s own decorators. The target-kind check runs after
  the join: it finds a fact type's `@annotate(...)` fact, recognizes
  `std.annotation.Annotate` by the qualified name the loader records
  (`DataDecl.standardName`), and reads the listed kinds from the written
  arguments, as the other fact passes read types from syntax. A value
  that a member line attaches is checked on that line. A newtype is a
  `.Newtype` target, and a value on a kind its limit omits is
  `decorator-target-kind` (D10);
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
  covered: `Hash` for `TypeId`, a type
  parameter bounded only by a subtrait of `Inspectable`, Inspectable keys in
  closure types and provider scopes, qualified printable names (the
  prototype has one module), and opaqueness (`TypeId { key: ... }` is
  constructible). A type parameter instantiated with `mut U` looks up
  implementations for `U`, and its Inspectable dictionary adds the inner
  `mut` when a composite key is built from it;
- executable `std.testing.assert` with source-order argument evaluation, plus
  `assert_equal` for supported scalar, string, tuple, list, optional, `Result`,
  and order-independent map values and for explicit nominal or bounded generic
  `Eq` implementations, with mandatory reasons and
  `missing-partial-eq` at unsupported types; `std.testing.snapshot` runs as
  a string `assert_equal` with a literal `expect` (no update run rewrites
  it), and `snapshot_file` is hd code in `lib/std/testing.hd` whose host
  function (`src/snapshots.ts`) compares the text with
  `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`, failing
  with `assertion-failed` (Testing T54) when it is missing or differs, except under
  `hd test --update`, which writes it (Testing T53); `Choices` and
  `Arbitrary` are hd code there too;
- `it_prop` and `it_prop_with` register one property test case, which the
  runner runs once per generated case in a fresh instance
  (`src/property-tests.ts`). Every `Choices` draw goes through the
  `prop_draw` host function, which records it; a failing case is shrunk
  by replaying shorter or smaller choice streams, and the report names
  the seed, the shrunk input's `Debug` text, and the shrunk stream.
  `cases`, `shrink`, and `hd test --seed N`, `--cases N`, and
  `--shrink N` cap the run. A case that `assume` discards does not count
  toward `cases`, and more than 10 × `cases` discards fail the property.
  The shrunk stream is saved, one draw per line, under
  `__regressions__/<module>/<test-slug>` (`src/snapshots.ts`) and
  replayed before new cases on the next run;
- `--test-layout test-module|integration` compiles a file as a test module
  (the conformance Test Layouts); both layouts are test modules, since
  the prototype has no separate integration view;
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
- strings backed by Wasm GC byte arrays. The prelude string methods
  (`len`, `trim`, `lower`, `split`, `replace`, `starts_with`) are hd code in
  `lib/std/text.hd` over three byte primitives; `lower` and `upper` call
  the host through the generic host-function boundary
  ([Compiler/Library Boundary](#compilerlibrary-boundary)). Every
  host-boundary decoder keeps a leading U+FEFF;
- non-suspending `defer` on normal completion, return, break, and continue;
- the `std.ops` operator traits
  ([Operator Traits](../spec/05-expressions.md#operator-traits)): an operator
  on primitive operands keeps its built-in code, and any other operand calls
  the left operand's implementation, found by the trait's qualified name
  (`HirTrait.standardName`), or its bound's through a supertrait. Compound
  assignment, `Index` and `IndexSet`, supertrait bindings such as
  `Add[Self, Out = Self]`, and the sealed `std.num` traits `Num`, `Integer`,
  and `Float` follow the same path. The primitive implementations are hd
  code whose bodies are the built-in operators. Floating `%` calls the
  host's `rem_f64`, JavaScript's truncated remainder;
- homogeneous `List[T]` literals, indexing, `len()`, and mutable `append()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut List[T]`;
- insertion-ordered `Map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut Map[K, V]`, and erased Wasm GC key/value storage.
  A key is an `i32`-like scalar, a string, or a non-generic declared type
  with `Eq` and `Hash` implementations (trait.hash.map-key), which the map
  compares with a wrapper of its `Eq`; a type parameter bounded by `Eq` and
  `Hash` keys a map built elsewhere;
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
Task combinator intrinsics, and strings and structural values in the
host-provider ABI, remain in later MVP slices. The compiler rejects syntax it
recognizes from those slices rather than assigning placeholder semantics;
unresolved `all!` and `race!` calls report `unsupported-task-combinator`.
Interpolation and `println` report `unsatisfied-trait-bound` when the displayed type
does not implement the canonical prelude trait.

## Standard Library

The toy standard library is hd source in the top-level
[`lib/std/`](../lib/std/) directory, next to `src/` as in Zig, one file per
module: `std.annotation`, `std.cmp`, `std.collections`, `std.hash`, `std.console`, `std.format`, `std.iter`, `std.num`, `std.ops`,
`std.option`, `std.process`, `std.result`, `std.testing`, `std.text`, and `std.time`. It
follows the draft in
[future-work/STDLIB.md](../future-work/STDLIB.md#core-layer) where the
specification allows; the open points are listed there under
[Questions For The Owner](../future-work/STDLIB.md#questions-for-the-owner).
`checker/standard-sources.ts` reads the files, and
`checker/standard-library.ts` joins what a program uses into the one module
the prototype compiles:

- a module's declarations are added when the program imports one of its
  names, as in `use std.cmp.{max, min}`, each under the local name or
  alias, and the rest under hidden names such as `__std_cmp_clamp`. A
  module's own `use std.<module>.<Name>` lines pull in that module the
  same way;
- an inherent implementation on a built-in type (`impl string:`,
  `impl[T] T?:`, `impl[T, E] Result[T, E]:`, `impl[T] List[T]:`,
  `impl i32:`) needs no `use`
  ([`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std)).
  Only the methods whose names the program selects with `.name` are added,
  to a fixed point over the added bodies; only `std` sources may declare
  them (`ImplDecl.standard`). The normative `List.map` and `T?.map` are
  among them;
- such a method's body adds only the module declarations it reaches, such
  as `std.text`'s byte primitives, not the whole module;
- a std trait's implementation for a built-in type, such as the primitive
  `impl Add[i32] for i32` of `std.ops` or `impl Num for i32` of `std.num`,
  is added only when the program, or a std declaration it gets, names the
  trait, so that `use std.ops.num_suffix` does not add every operator
  implementation;
- every added declaration's span is the `use` that brought it in, or the
  program's span.

What it provides:

| Module | Contents |
| --- | --- |
| `std.annotation` | the shape types (`DataShape`, `FieldShape`, `TypeShape`, ...), `ShapeMetadata`, and `TypeShape.is_optional`; `checker/shapes.ts` generates the builders that `shape[T]()` and `shape_of(f)` call. `Target`, `Annotate`, and `annotate`, which limit a fact type's target kinds |
| `std.hash` | `Hash` and `Hasher` (prelude names), and `Hash` for `string`, `bool`, and every integer type; no standard hasher, which the specification does not name |
| `std.option` | on `T?`: `map`, `unwrap_or`, `ok_or`, `is_some`, `is_none`, `expect` |
| `std.result` | on `Result[T, E]`: `map_ok`, `map_err`, `ok`, `err`, `is_ok`, `unwrap_or`, `expect` |
| `std.collections` | on `List[T]`: `map`, `filter`, `first`, `last`, `reversed`, `sorted_by` (stable), `chunks`, `zip` |
| `std.text` | on `string`: `is_empty`, `ends_with`, `contains`, `find`, `upper`, `trim_start`, `trim_end`, `strip_prefix`, `strip_suffix`, `lines`, `repeat`; `join`, `StringBuilder`; the prefix `r` and its helpers `interpolate`, `process_escapes`, and `EscapeError` |
| `std.iter` | `range`, and the adapters `map_each`, `filter`, `take`, `enumerate`, `collect`, `fold` as free functions |
| `std.cmp` | `min`, `max`, `clamp`, `Reverse[T]` |
| `std.num` | the sealed `Num`, `Integer`, and `Float`, implemented for every primitive number type; on `i32` and `i64`: `checked_*`, `wrapping_add`, `wrapping_sub`, `saturating_*`, `abs_diff`, `count_ones`, `leading_zeros`; on `f64`: `is_nan`, `is_finite`; `parse_i32`, `parse_i64`, `ParseNumberError` |
| `std.time` | `Duration` with `milliseconds`, `seconds`, `as_milliseconds`; the suffix functions `ms`, `s`, `min`, `h` |
| `std.console` | `ConsoleInput`, and the recording `BufferConsole` with `new` and `output` |
| `std.process` | `ExitCode`, `Termination`; `Process`, `Command`, `Output`, `ProcessError`, and the deterministic `ScriptedProcess` |
| `std.ops` | the twelve operator traits, the ten assign traits, `Index`, and `IndexSet`, with the primitive implementations of the operator traits; `NumSuffix` and `num_suffix`, the literal-suffix marker; `StrPrefix`, `str_prefix`, and `Template` |
| `std.format` | `DebugWriter` and the builders `DebugStruct`, `DebugTuple`, `DebugList`, `DebugMap`; the prelude `debug`; `Debug` for the primitives, `List`, `T?`, `Result`, and pairs |
| `std.testing` | `Choices`, `Arbitrary` (for the primitives and `string`), `snapshot_file`; the rest of `std.testing` is checked by the compiler |

The prelude `string` methods live in `std.text` too, and `lower` and
`upper` are backed by the host. Prototype limits: the `std.iter` adapters work on the built-in list and map cursors
(the prototype's `mut Iterator[T]`) and collect eagerly, except `take`;
there is no `chars`, `to_utf8`, or `from_utf8` (the byte primitives are
private to `std.text`), no `parse_f64`, `wrapping_mul`, or `Float` rounding methods, no
`Integer` or `Float` trait, no `Set` (the specification does not define it,
and a map built in generic code has no key equality for a type-parameter
key, so a generic `Set.new()` could not create its map), and no host `ConsoleInput`; a `BufferConsole` records both direct
`write_line!` calls and, outside a driver, `println` (MHP-1). `test/std/*.hd` tests each module through `hd test`, and
the playground's `std` example uses several.

## Compiler/Library Boundary

The compiler should know the language, not the library. A capability
such as `fs` or `net`, or a string algorithm, belongs in `lib/std` hd code
plus, where it touches the outside world, a host-side function. It should
not need a HIR node, a checker case, or a hand-written WAT helper. This
section lists where the prototype still breaks that rule, and the plan.

### Boundary Mechanisms

There are two ways for `lib/std` to reach below hd code. Both are
prototype-internal: the specification has no syntax for a library to
declare a host function (a question in
[RUNTIME_AND_LIBRARY.md](../future-work/RUNTIME_AND_LIBRARY.md#prototype-host-function-declarations)).

1. **Intrinsic functions.** A `lib/std` function preceded by
   `@intrinsic("name")` is an ordinary declaration whose body the compiler
   supplies. The standard-library loader turns the line into
   `FunctionDecl.intrinsic` (`checker/standard-library.ts`). The same line in
   user code is an ordinary decorator whose value calls an undeclared
   `intrinsic` (`unknown-name`), so only `lib/std` can use it. The written body (`panic("intrinsic")`)
   type-checks and is never emitted. Calls are ordinary calls.
   - A **runtime primitive** is a few Wasm instructions over the runtime's
     own value layout, listed in `emitter/intrinsics.ts`: today
     `string_byte_len`, `string_byte_at`, and `string_byte_slice`.
   - Every other name is a **host function**, imported as `hd`
     `host:<name>` through one generic path. Scalars cross as Wasm numbers,
     and a `string` crosses as a host handle that `emitter/runtime/boundary.wat`
     copies byte by byte. The host looks the name up in
     `src/host-functions.ts` (today `string_lower`, `string_upper`, and `string_from_scalar`), or
     in the runner's `hostFunctions` (`snapshot_file_check`, `src/snapshots.ts`).
2. **Host capability traits.** A capability is a trait with suspending
   methods (spec/11 and
   [RUNTIME_AND_LIBRARY.md](../future-work/RUNTIME_AND_LIBRARY.md#capabilities-and-sandbox)).
   A host-bound trait gets a provider value built by
   `emitter/host-providers.ts`: each method's call goes out through
   generic per-method `host_<trait>_<method>_*` imports with the same
   boundary values, and the host answers through one
   `hostSuspensionInvoke` callback keyed by trait and method name, with
   record and replay. A method may also return `Result[T, E]` with a
   boundary or `void` `T`: the tag crosses first, then the active side's
   payload. An `E` that is not a boundary type, such as `ConsoleError`,
   can be named but not built, so the host may not report `.Err` for it.
   The host console is a built-in entry, `Console.write_line`, in
   `HOST_PROVIDERS` (`src/host-functions.ts`); `UNRECORDED_PROVIDERS`
   keeps its calls out of record and replay, as before.

Adding a pure host-backed std function needs its `lib/std` declaration and
one entry in `src/host-functions.ts`. Adding a capability such as `FsRead`
needs its trait in `lib/std` and a host implementation behind
`hostSuspensionInvoke`; neither needs a HIR node or a checker case.

### Audit

The special cases found on 2026-09-28, grouped by where they live. "Done"
marks what this refactor removed.

| Area | Special case | Kind | Status |
| --- | --- | --- | --- |
| HIR | `string-length`, `string-transform` (`trim`, `lower`, `upper`), `string-split`, `string-replace`, `string-starts-with` | string library | Done: hd code in `lib/std/text.hd` on three byte primitives; `lower` and `upper` are host functions |
| Checker | `checkStringMemberCall`: `len`, `trim`, `lower`, `upper`, `split`, `replace`, `starts_with` by name | string library | Done: ordinary `impl string:` methods |
| WAT runtime | `string-split.wat`, `string-transform.wat`, `$hd.string_len`, `$hd.string_starts_with` | string library | Done: removed; `$hd.string_slice` stays as the slice primitive |
| Host glue | `string_transform_begin`, `_input`, `_output` imports, `trimWhiteSpace` | string library | Done: `trim` is hd code; case mapping goes through `host:` |
| HIR | `console-print` (`println`) | capability | Done: `println` is hd code in `lib/std/console.hd` |
| Checker | `println` by name | capability | Done: an ordinary std function; std may declare a prelude name (`FunctionDecl.standard`) |
| Emitter | `emitPrintln` (`$hd.println`) | capability | Done: `println` drives `write_line!` with `block_on` |
| Host glue | `println_pending`, `println_error` imports | capability | Done: removed; the panics are ordinary `std` panics |
| Checker | `Console` trait declared in TypeScript (`program-types.ts`); `ConsoleError` as a primitive type name (`shared.ts`, `context.ts`, `termination.ts`) | std declarations | Remains: see Console below |
| Emitter | `emitConsole` (a hand-written host `Console` provider), `console.wat` (`$hd.console_print`) | capability | Done: the generic capability bridge, with `Result` results |
| Host glue | `console_byte` import | capability | Done: `Console.write_line` in `HOST_PROVIDERS`, left out of record and replay |
| Checker | `validateHostCapabilities` skipped `Console` | capability | Done: `Console` passes the same boundary check as any host capability |
| HIR | `assert`, `assert-equal` | `std.testing` | Remains: `assert_equal` needs the compiler's equality strategies; see below |
| HIR | `snapshot-file` | `std.testing` | Done: `snapshot_file` is hd code in `lib/std/testing.hd` with a host function |
| HIR | `each-row-index`, `each-row-count`, `test-timeout` | test runner hooks | Remains: runner protocol, not library code |
| HIR | `debug-render` | `std.format` | Done: `debug`, `DebugWriter`, and its builders are hd code in `lib/std/format.hd` |
| HIR | `list-*`, `map-*`, `iterator-next` | built-in `List` and `Map` | Remains: the collection types are built into the runtime layout |
| HIR | `inspect-type-id`, `inspect-downcast` | `std.inspect` | Remains: runtime type identity is a compiler service |
| Checker | `block_on`, `all!`, `race!`, `shape`, `shape_of`, `downcast_val` | spec-named intrinsics | Remains: the specification names them compiler intrinsics. `shape` and `shape_of` lower to calls of generated hd builders over `lib/std/annotation.hd`, with no HIR node |
| Checker | `Duration` for test `timeout`, `ExitCode` and `Termination` for entry results (`standard-traits.ts`, `termination.ts`) | `std.time`, `std.process` | Remains: language hooks that name a std type; the declarations are already hd |
| Checker | `Display`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Iterator`, `Iterable`, `Any`, `Debug`, `Ordering` declared in TypeScript | prelude declarations | Remains: operators, `for`, and interpolation are wired to them |
| Emitter | `float.wat` and the `format_f64`, `format_f32`, `pow_f64`, and `rem_f64` imports | float display, `**`, and floating `%` | Remains: operator and interpolation support |

Counts: the HIR expression union had 92 kinds, of which 15 were library-
or capability-specific. The string step removed 5, the `println` step 1,
and the `debug` and `snapshot_file` step 2, leaving 84 kinds, 7 of them
specific: the test-runner hooks, `assert`, `assert_equal`,
and `std.inspect` rows above. No capability has a HIR node now.

### Console

`println` is hd code in `lib/std/console.hd`
([`module.prelude.println`](../spec/10-modules.md#r-module.prelude.println)).
It calls `write_line` without `!`, which makes a stored suspension, and
drives it with `std.task.block_on`, so it inherits all of `block_on`'s
rules with no checker case of its own
([MHP follow-ups](../future-work/OPEN_ISSUES.md#mutable-host-providers)).
Its `.Err` panic is an ordinary `panic` call. `std.task` is a
compiler-provided module, not a `lib/std` file: the loader keeps a std
module's `use std.task.block_on` line as a program `use` under a hidden
name, so the call is an ordinary `block_on` call. The loader adds
`println` under its own name when a program mentions it, because
`println` is a prelude name.

The compiled module is the entry module, so its top-level statements may
call `block_on` and `println`
([`req.drive.block-on.forbidden-contexts`](../spec/11-requirements-and-suspension.md#r-req.drive.block-on.forbidden-contexts)
forbids only non-entry module initialization). A linked package shares
one namespace, so the prototype cannot reject a driver in another
module's initialization.

The host console is a built-in entry of the generic capability bridge.
Its calls stay out of record and replay (`UNRECORDED_PROVIDERS`), so
`hd replay` prints console lines again rather than reading them back.
Whether a replay should capture them is an owner question in
[OPEN_ISSUES.md](../future-work/OPEN_ISSUES.md#mutable-host-providers).

What remains:

1. Declare the `Console` trait in `lib/std/console.hd` instead of
   `program-types.ts`. Its trait index is fixed today (the last built-in
   trait), which `lowerRunTimeGaps` relies on to drop an unused `Console`.
2. `ConsoleError` stays a TypeScript type name until its variants and
   constructor are settled with the other std error types.

## Layout

- `lexer.ts` and `ast.ts` define the small source-frontend stages.
- `parser/` builds the AST and exposes its public API from `parser/index.ts`.
- `checker/` resolves names and produces the typed nodes in `hir.ts`.
- `emitter/` lowers HIR to readable WAT and exposes only `emitter/index.ts`.
- `checker/standard-sources.ts` reads the toy standard library's hd
  sources from the top-level `lib/std/`; `checker/standard-library.ts` joins
  them into a program.
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
