# hd-lang MVP Compiler

This directory contains the executable Wasm GC MVP described in the archived
MVP_IMPLEMENTATION_PLAN.md. The implementation is
deliberately incremental: accepted programs compile to validated Wasm GC, and
features outside the current slice receive stable diagnostics.

Small pipeline stages remain direct modules such as `lexer.ts`, `ast.ts`,
`hir.ts`, and `wasm.ts`. Larger stages use same-named folders (`parser/`,
`checker/`, and `emitter/`). Each folder exposes its public surface only from
`index.ts`; consumers do not import its internal files.

## Run It

The repository pins Node 24.19.0 and its dependencies through
`pnpm-lock.yaml`.

```sh
pnpm install
pnpm run toolchain:gate
pnpm run lint
pnpm run format:check
pnpm run test:portable
pnpm test
pnpm run hd help
pnpm run hd help test
pnpm run hd check examples/core.hd
pnpm run hd test spec/conformance/runtime/valid/defer-order.hd
pnpm run hd test test/std
pnpm run hd build --wat examples/core.hd
pnpm run hd run examples/core.hd
pnpm run hd repl
pnpm run hd check --format json examples/core.hd
pnpm run hd explain unknown-data-field
pnpm run hd doc main examples/core.hd
pnpm run hd debug parse spec/conformance/parse/valid/layout.hd
pnpm run hd debug hir examples/core.hd
pnpm run check
```

### Commands

`hd`, `hd --help`, and `hd help` list the commands; `hd help COMMAND` (or
`hd COMMAND --help`) prints one command's usage and flags. `cli-args.ts`
holds the command table.

```text
hd build [--wat] FILE
hd run   [--entry NAME] FILE
hd test  [--update] [--seed N] [--cases N] [--shrink N] [FILE|DIR]
hd check [--tests] FILE
hd explain CODE      hd doc NAME [FILE|PKG]      hd def NAME [FILE|PKG]
hd repl              hd help [COMMAND]           hd debug parse|hir FILE
```

- Each command owns its flags. `--format text|json` is the only global flag,
  and it may come before or after the command. A flag given to a command
  that does not own it exits 2 and names the commands that do.
- Flags may come before or after the operands; `--` ends the flags.
- A usage error prints to stderr and exits 2.
- `hd debug parse` and `hd debug hir` print internal compiler output.
  `hd parse FILE` stays as a hidden spelling of `hd debug parse`, because
  the conformance command contract names it
  ([Command Contract](../spec/conformance/README.md#command-contract)).
- Help lists the conformance fixture flags apart from the others:
  `--profile NAME` (on `build`, `run`, `test`, `check`, and `debug hir`),
  `--scenario NAME` and `--pending-function NAME` (on `test`), and
  `--test-layout`, `--package-tree`, and `--package-path` (on `test` and
  `check`). The last three are temporary: a package's layout should come
  from its `hd.toml`, which the prototype does not read yet.
- `hd test DIR` tests a package (a directory with `hd.toml` or `src/`) one
  module at a time: each file under `src/` and `tests/` is linked with the
  rest of the package and runs only its own test cases. Any other directory
  has each `.hd` file in it tested on its own. `hd test` with no path tests
  the package that holds the current directory, or else the current
  directory. An error in a module that several modules link prints once.
- `hd run`, `hd build`, `hd check`, and `hd test` on a FILE link FILE with
  its package, so `pkg`, `self`, and `super` uses between modules resolve.
  The package is the one that holds FILE: the nearest directory with
  `hd.toml`, else the parent of the nearest `src/`, or of a `tests/` beside
  a `src/`. A FILE outside the package's `src/` and `tests/`, or outside any
  package, compiles on its own. `--package-tree` and `--test-layout` turn
  this off.

`hd run` runs the public `main` or `main!`; a module without one runs its
initialization and exits 0, while `--entry NAME` must name a function.
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
linked with `pnpm link`.

## Agent Queries

hd is written mostly by coding agents, so the CLI answers questions about a
program by name and in JSON, not by file position
([roadmap](../future-work/ROADMAP.md#direction)).
Every command below also has the default `--format text`, which is the only
format a person needs.

### Machine-Readable Diagnostics

`--format json` is the one global flag. It matters for every command that
compiles a file: `check`, `test`, `run`, `build`, and `debug`. It changes only the diagnostic stream: each
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
- `List[T]` varargs (`values...: List[T]`), whose type is the collected
  list, with positional values, positional list spread, and named-list
  supply across ordinary, generic, suspending, static-trait, and
  dynamic-trait calls;
- function types whose inputs end in the rest element `List[T]...`, as
  `fn(i32, List[i32]...) -> i32`, so a function value keeps its vararg, and
  indirect calls using the same `List[T]` ABI;
- tuple types with a rest element `(A, List[T]...)`, whose value holds the
  fixed elements and then one `List[T]`; tuple expressions collect trailing
  elements into an expected rest element, or end in a list spread
  `(a, xs...)`; spread patterns `(a, xs...)` in `let`, `for` headers, and
  `match` arms. A rest tuple
  compares with `==` and `<` element by element, its rest element as its
  list, and `lib/std/format.hd` declares its `Debug` and `Display`, with the
  rest element's items written inline, for at most 11 fixed elements;
- tuple-typed and `Tuple`-bounded varargs (`args...: (i32, string)`,
  `args...: Args`), which take one tuple input: a call passes its trailing
  arguments as the tuple expression of them, and a `Tuple`-bounded type
  parameter is solved from that tuple;
- `Fn[Args, O, R]` and `SuspendFn[Args, O, R]` with `Args < Tuple`, whose
  type text is `fn(*Args)->O$R`: one erased input that holds the inputs
  tuple. Substituting a tuple for `Args` gives the plain function type,
  inference solves `Args` as the tuple of a function value's inputs, and a
  closure adapter unpacks the tuple into the actual parameters or packs
  them into it. Implementations over such a target match any arity, and
  their row parameter is solved from the receiver;
- a tuple spread into fixed parameters, as `add(pair...)` or `g(t...)`:
  the operand is evaluated once into hidden locals, must have the same type
  as the tuple of the remaining inputs (rest element included), and passes
  each fixed element, and its rest element's list at the vararg. Into
  `*Args` it passes the operand whole;
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
- explicit generic call arguments with per-slot `_` inference for
  functions and inherent methods, where a short list infers its omitted
  trailing slots and a long one is `argument-count`, plus indented
  zero-argument trailing callback blocks. In an expression the list follows
  `::`, as in `first::[string](xs)`, `fetch!::[User](key)`,
  `Box::[i32] { ... }`, and `Add::[i32]::add(a, b)`; `[` after an operand
  always indexes, and `Box[i32] { ... }` or `Add[i32]::add` is `syntax-error`;
- type-argument defaults (`[T < Bound = Default]`) on functions, methods,
  data types, enums, traits, and `type` declarations, never on an
  implementation header (`syntax-error`). `type-defaults.ts`
  fills the slots a written type omits, with the earlier arguments and
  `Self` substituted (`Self` is the bounded parameter, the implementation's
  target, or the trait's own `Self`), before aliases expand; a written type
  that omits a slot without a default is `partial-generic-arguments`, and a
  trait value type whose default names `Self` is too. It also reports
  `default-order`, `binding-not-yet-visible`, a row default for a type
  parameter (`generic-kind-mismatch`), and, after implementations are
  prepared, a default naming a data type or enum that does not implement
  its bound (`unsatisfied-trait-bound`). A
  call, a generic function value, a data literal, and a call of a trait
  method through a bound apply a default only to what inference left
  unsolved (`applyGenericDefaults`), and an implementation method must
  repeat its trait method's defaults (`trait-method-signature`). The
  `std.ops` binary operator traits default `Rhs = Self`. Bounds on the
  parameters of a data type, enum, or trait are parsed, and the prototype
  checks them only against a default and, for `Eq & Hash`, as map keys;
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
  canonical `Display` dispatch for concrete and generic implementations
  (such as `lib/std`'s tuple `Display`, with its bounds' dictionaries), generic bounds,
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
  with `block_on` (MHP-1). Under `main!` or a test body, `block_on`
  clears the outer driver's active flag, drives only its own suspension,
  and restores the flag (req.drive.block-on.inner-only), so `println`
  writes there. It is `suspension-forbidden-context` in a
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
- list and insertion-ordered map `for` iteration with value-producing
  `for ... else`. A `for` loop or comprehension clause takes an irrefutable
  pattern: a name or a tuple of names binds directly, and any other pattern
  binds a hidden item that a one-arm `match` destructures (in a
  comprehension, through a one-element list per item), so a refutable one is
  `refutable-let-pattern` and the bare `for a, b in` is `syntax-error`;
- eager list and map comprehensions with ordered nested clauses, conditional
  filters, lexical clause bindings, and duplicate map-key replacement. A bang
  call in one is valid where it is valid in the loops the comprehension
  abbreviates; in a suspending body such a comprehension lowers to those
  loops, so each call completes before the next clause;
- right-associative single-name binding expressions with enclosing-scope
  visibility, readonly inferred bindings, and flow-sensitive initialization
  across short-circuit conditions. `[a, b := v]` and `(a, b := v)` end in a
  binding; a name list before `:=` inside an expression is `syntax-error`;
- pipe expressions `value |> step` with leading-`|>` continuation lines. The
  parser checks each step's own `_` placeholders, bare steps, and one-line
  steps; the checker binds the value to `_` and lowers the pipe to a one-arm
  match, so the value is evaluated first. A bare step `path` is the call
  `path(_)`;
- `let` bindings that infer the readonly view, and `let mut` bindings,
  per name in a parenthesized `let (mut a, b)` list, that infer `mut T`,
  reject a readonly value (`mutable-upgrade`), a readonly annotation
  (`let-mut-readonly-type`), or a primitive (`mut-on-primitive`), warn on
  a redundant `mut` before a name, alone or in a list, whose annotated type
  is already `mut` (`redundant-let-mut`, with a fix-it that deletes it), and use a
  non-generic data literal as `mut T`. `:=` binds one name: a pattern before
  it, such as `(a, b) := pair`, is `missing-let` with a fix-it that writes
  `let` and `=`, and the bare `let a, b` and `a, b :=` lists are
  `syntax-error`s whose fix-it adds the parentheses. A `let` takes any
  `match` pattern, with `mut` before each name it binds, and an optional
  `else:` block. A name or a tuple of names binds directly and `let _ = v`
  discards; any other pattern, or any `let` with an else block, binds a
  hidden item and hands the pattern's names to an ordinary `let` through a
  match, `let (a, b) = match item: P => (a, b); _ => else-block`. So a
  refutable pattern without else is `refutable-let-pattern`, an else block
  after an irrefutable one is `unreachable-match-arm`, and an else block
  that may complete is `let-else-falls-through`. A `mut` data subject
  matches as its data type, and a direct `mut U` field of a readonly
  subject binds as `U`. A primitive type
  written `mut`, as in `mut i32`, is `mut-on-primitive`. In a `mut self`
  method of an implementation for a primitive, `self` has the plain type,
  and a call, `Type::method` reference, or qualified call of it needs no
  mutable access;
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
  GC environments for direct and transitive captures; a closure captures no
  provider from an enclosing `$.with`, so each key its body uses stays in its
  row and resolves at each call, and its `$.with` keys are compared for
  collisions only with its own row and its own `$.with` blocks (the declared
  row's keys name the enclosing generics); a provider value bound by `$.use` is
  captured like any local and outlives its scope; a captured `let` is a
  shared heap cell, and every closure may assign it and keep mutable
  captures (`mut fn` is a syntax error); a generic function used as a value is
  instantiated from explicit type arguments or the expected function type;
  function types convert by declared variance (permission changes only),
  are implementation targets owned by the standard library, reject a direct
  `is`, and may be spelled `Fn[...]` and `SuspendFn[...]` when imported
  from `std.function`;
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
- associated type bindings (`associated-bindings.ts`): a binding may name an
  associated type the trait reaches through its supertraits, and a name two
  reachable declarations share is `ambiguous-associated-type`, as is a
  projection `I::Item` that two bounds on `I` both reach, bound or not. A
  trait value type and a requirement key may bind associated types
  (`Supplier[Item = i32]`, `$ Store[Item = User]`), rendered
  `Supplier[Item=i32]` with the bindings after the positional arguments in
  name order, so two spellings are one type or key. A trait value type is
  dynamically safe only when it binds every associated type its trait
  reaches, and a key that leaves one unbound is
  `trait-not-dynamically-safe`. Through such a value each `Self::Item` is
  the bound type; a concrete value converts, and a provider installs, only
  when its implementation binds the same types (`type-mismatch`); widening
  keeps the bindings; and the value satisfies a bound on its trait with the
  projection equal to its binding. A binding on a type that is not a trait
  is `unknown-associated-type`, and one in an implementation header or a
  trait-qualified call is a `syntax-error`;
- concrete requirement rows with hidden `externref` provider threading;
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
  for a function's row parameters, as in `provide::[$ Db + Log](job)`;
- row subsumption: a function value with a narrower concrete row fits a
  wider function type through the callable adapter. A value is not widened
  into a row that holds a row parameter it lacks, which least-row inference
  solves instead. A list or map literal with no expected type gives its
  function values the union of their rows, and a spread contributes its
  list's element row: a part whose row is smaller is checked again against
  the union, a spread `xs` as `[for x in xs => x]` (RU12). Diagnostics
  print the union's expanded keys rather than the rows as written. `if`
  branches, `match` arms, and inferred closure and function results take
  the union too (RU15), and a branch or arm type mismatch is
  `no-common-type`. A function type whose result is a function type
  parenthesizes that result in its type string, as in
  `fn(bool)->(fn()->string$Db)`, so the inner row stays the inner
  function's;
- erased generic marker traits as provider keys, with call-site substitution
  and pre-erasure collision checking;
- erased generic functions with call-site type inference, Wasm GC boxing for
  primitive values, inference through optional and `Result` types, and
  higher-order callable adapters for erased type and requirement-row ABIs.
  Arguments that solve one type parameter, `assert_equal`'s two values
  included, must have one type up to `mut`: a numeric widening between
  them is `type-mismatch`, and a trait-value conversion `no-common-type`
  (`types.generic.infer.join`); a numeric literal takes the solved type.
  Boxing is a toy shortcut: the
  [implementation model](../spec/04-type-system.md#shapes-and-generic-code)
  gives each value layout its own body and keeps values unboxed in generic
  code and containers;
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
  on a generic target, as `Box::[Point]::name()`, the target's arguments
  solve the implementation's parameters, which the call's arguments need
  not mention; `T::function()` on a type parameter calls through the
  bound's dictionary;
- method references (`checker/method-references.ts`): `Type::method`,
  `Trait::method`, and `T::method` are checked as the closure that calls the
  member, receiver first, with type arguments written after the name or
  solved from the expected function type (a trait reference's `Self`
  included). `value::method` evaluates the receiver once and closes over it.
  A called reference is an ordinary call: `value::name(...)` is a method
  call, and `Type::method(receiver, ...)` calls the method on its first
  argument. `to_string` on a primitive calls its built-in `Display`. A
  non-suspending closure shares its enclosing function's generic parameters
  and bounds, and its environment keeps the bound dictionaries after its
  captures, so a `T::method` reference calls through the bound; a
  suspending closure still reaches no bound dictionary;
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
- concrete and bounded generic `Eq` and `PartialOrd` dispatch: the compiler
  compares primitives, and calls every other type's implementation,
  generic ones too, passing the dictionaries of the implementation's
  bounds. `std.cmp` implements `Eq` for lists, optionals, `Result`, and
  maps, and `PartialOrd` and `Ord` for lists and optionals (`.None` first),
  in hd, and tuples through its tuple templates; the loader adds them when
  a program, or std code it gets, mentions a comparison trait, a comparison
  operator, or `assert_equal`. Primitives also satisfy `Eq` and
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
  and by the CLI for a package tree): `pkg`, `self`, and `super` uses between the modules of one
  package resolve to public declarations and `pub use` re-exports, and the
  modules reachable from the entry are joined into one program in
  initialization order. A `*_test.hd` test module joins as a `tests:`
  block, and a test build links every test module; `hd test FILE` parses a
  `*_test.hd` file as a test module, whose top level is test position. A
  file under `tests/` is the integration test module `tests.<path>`, which
  links like a test module: it uses the library through `pkg` (public
  declarations, library modules only) and other integration test modules
  through the `tests` root or `self`, whose base is the test root. The
  shared namespace does not hide library privates from it. Files
  of one folder may use each other in a loop; a loop of folders is
  `folder-cycle`, reported once per tangle with one shortest folder loop,
  each edge's `use` line, the tangle size, and an `x.hd` to `x/mod.hd`
  fix-it; uses in test code make no folder edge
  (10-modules.md#dependency-cycles). Each `pub use` whose chain returns to
  a module it passed is `re-export-loop`, and so is a plain use through
  such a loop. Modules that use each other form one
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
  parser desugars them to the call `ms(250)`; `-250ms` is the ordinary
  negation `-(ms(250))`. Radix literals take no suffix, and a reserved-word
  suffix such as `5else` is `invalid-token`. The checker resolves the
  suffix among module-scope functions only, requires a function marked
  `@num_suffix` (`FunctionDecl.numSuffix`, set after the std join by
  `checker/decorators.ts` from a value of `std.ops.NumSuffix`), and checks
  the ordinary call, so a generic or provider-needing suffix function
  follows the ordinary rules (`expr.literal-fn.ordinary-call`). The marker
  is a typed fact (see typed facts below), so the marked function's shape
  is checked at the decorator, by `num_suffix`'s signature in
  `lib/std/ops.hd`; an
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
  converts to the template's `T` like an argument. The prefix shape is
  checked at the decorator, by `str_prefix`'s signature, as for suffixes;
  an unmarked function at the string is
  `invalid-string-prefix`. `std.text` declares `r`, `interpolate`, and
  `process_escapes` (which returns `Result[string, EscapeError]` with the byte offset of
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
  describers, and sources, declared in hd when imported (under hidden
  names when the program declares a type of the same name, as a `data Key`).
  A template is checked once (`annot.template.checked`): each method becomes
  a generic function over the template's `T`, bounded by a hidden trait that
  stands for `T`'s `Structure` in that template, with `hd_name`, `hd_facts`,
  and one method per `walk`, `describe`, or `build` call site. Each
  derivation implements that trait for its target, calling generated
  traversal functions specialized to the target and to the walker,
  describer, or source type, and implements the derived trait by calling
  the template's functions with the target as `T`. So a derivation adds
  only its traversals and handles, never a copy of the template body. The
  template must hold the walker, describer, or source in a local declared
  with its type (`unsupported-derivation` otherwise). A plain handle of a
  non-generic target is a module constant, and derivations of one target
  without member lines share their handles. `T::name()` returns the
  target's declared name (`annot.structure.name`). Inside a template,
  `Structure::f(...)` is `T::f(...)`, and a call qualified by the derived trait, as
  `Named::name()`, calls the target's own implementation
  (`annot.template.qualified-self`). A walker's `member` and `rest` may strengthen their bounds; their dictionary
  entries trap, since only generated code calls them, concretely.
  A tuple template, `impl[T < Tuple] Trait for T by Structure`
  (`annot.template.tuple.*`), is compiled the same way and instantiated
  once per tuple shape, its number of fixed elements and whether it has a
  rest element, as a generic implementation over the element types, such
  as `impl[hd_E0 < Eq, hd_E1 < Eq] Eq for (hd_E0, hd_E1)`
  (`checker/tuple-templates.ts`). Shapes need no inferred types: the pass
  reads them, and the names a program mentions, from the program as the
  std join will declare it, without std implementation bodies. It
  instantiates the program's own tuple templates for every shape, and
  std's (`Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug`, `Display`, `Default`)
  for the traits mentioned, with their supertraits. A rest member is one
  `List[T]` member; its item type takes the bound of the walker's `rest`,
  or of the trait's `List[T]` implementation, and a shape with neither,
  as `Hash` has, gets no instance. Generated `walk` calls `rest` only on a
  walker that implements it, and `member` otherwise, as `Walker.rest`'s
  default does; the prototype's default body itself panics. A tuple's
  `build` returns `Self`, since a tuple takes no `mut`. A hand-written
  implementation of such a trait for a tuple type is `overlapping-impl`.
  A map key whose `Eq` is a generic implementation, as a tuple's is,
  compares through that implementation's dictionary, and the key bound
  `Map[K < Eq & Hash, V]` is checked through generic implementations and
  their bounds (`checker/map-keys.ts`). The checks
  of the chapter's diagnostics (`underivable-trait`, `misplaced-derivation`,
  `marker-template`, `invalid-member-line`, `duplicate-fact`,
  `omitted-member-without-default`, `member-not-derivable`,
  `generic-member-call`, `newtype-derivation-self`, `gadt-derivation`, the
  two warnings, the `structure-variant-mismatch` panic, and
  `suspension-forbidden-context` in facts) are implemented. Gaps: `Facts`
  holds `Inspectable` values rather than `Any`: each generated fact list
  erases its values through the checker intrinsic `hd__structure_fact`,
  which names any type, a function type by its text, so a user's
  `facts.items` may hold such a value; a fact's concrete type for `duplicate-fact` is read from
  syntax (a data literal or a call's declared result), for member lines and
  for declaration facts alike (M25); `VariantInfo.shared`
  is always empty; a build handle's `get` returns the declared type whatever
  its argument's permission; a newtype forwards only through the receiver
  and plain `Self`; `@derive(Eq)`, `PartialOrd`, `Ord`, and `Hash`
  instantiate the std templates in `lib/std/cmp.hd` and `lib/std/hash.hd`,
  read from the std source as `Arbitrary`'s is. An enum orders by variant
  index first, so `PartialOrd` and `Ord` first walk `other` to read its
  index. A newtype calls its base type's method through a bounded helper,
  and `mixed-derived-law` (`checker/derive-intrinsics.ts`) checks the law
  partners; `==` and `<` use a generic implementation such as a derived
  `impl[T < Eq] Eq for Box[T]`. `@derive(Debug)` generates
  builder calls in Rust's mapping (Testing T53, T54): `debug_struct` for a
  data type, even a fieldless one, and a variant with named payload fields,
  `debug_tuple` for a variant with positional ones, and `write` of a
  payload-free variant's name. A variant that mixes both uses
  `debug_struct`, naming a positional field `_0`, `_1`, and so on. A field
  a derivation compares or hashes without the trait is
  `derive-field-missing-trait` at the field (at the base type for a newtype), and a use whose added bound
  fails is `missing-derived-bound`. `Debug` is a prelude trait; `DebugWriter`, its
  builders, the prelude `debug`, and `std`'s `Debug` implementations for
  the primitives, `List`, `T?`, `Result`, and tuples are hd code in
  `lib/std/format.hd`, whose writer is always compact. The checker still
  accepts `Debug` for `Map`, which renders no text; the drift and unused-fact warnings treat
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
  read by `facts_of(f).find::[M]()` (spec/14-annotations.md#function-facts).
  `checker/function-facts.ts` gives each function that a `facts_of` call
  names a generated `Facts` builder; the checker lowers a call whose
  argument names that function, not a local, to the builder's call, and any
  other argument, or `facts_of` as a value, is `invalid-facts-of-target`.
  Importing `facts_of` declares `std.structure`, since each call returns
  its `Facts`; the written result in `lib/std/annotation.hd` is `Any`, so
  modules that only depend on `std.annotation`, such as `std.ops`, need not
  declare `std.structure`. A module-qualified argument is not supported,
  since the prototype compiles one module. A bare decorator name of a function
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
- typed facts (spec/14-annotations.md#member-typed-facts), in
  `checker/typed-facts.ts`, after trait-less blocks are folded in and
  before derivations read the facts: `@annotate::[F](...)` on a data type
  or enum makes it a typed fact type, its argument must be one of the
  type's parameters (`type-mismatch` at the decorator), and the pass then
  drops the argument. `lib/std`'s `annotate[T = Any]` takes the ordinary
  default, so `@annotate(...)` without one is an untyped fact type.
  A typed fact on any target but a field or a module-level function is
  `decorator-target-kind`. Each value `v` on a field, or on a derivation
  block's member line for one, or on a module-level function becomes
  `hd__typed_fact_N::[X, _](v)`, the call of a generated identity
  function over the fact type's parameters and bounds, so ordinary call
  checking does the `let f: D[X] = v` check: `X` is the expected type,
  the other slots are inferred, and `D`'s bounds are checked, all at the
  decorator or line. `X` is the field's written type or the function's
  signature type with its `!` and row. A generic target's bounded
  parameters become parameters of the fact's check function, with their
  bounds and the bounds' supertraits, and unbounded ones become `Any`; a
  derivation's facts function holds such a fact as its value alone, since
  it has no generic scope. A fact type of `std`, such as `NumSuffix` or
  `With`, is found from the `std` function that the value calls.
  `h.fact::[M]()` on a handle is `std.structure`'s `Field.fact`, which
  finds the member's fact of exactly type `M`; it does not check that
  `M`'s target argument is the handle's `F`. Each handle holds a hidden
  `hd_witness: Inspectable`, built by the checker intrinsic
  `hd__structure_witness::[F]()` from the member's type, whatever it is.
  When `F` has no runtime identity of its own, the checker reads that
  witness at the `h.fact` call as `F`'s Inspectable dictionary
  (annot.handle.fact.key), so `M`'s other parameters still need one. The
  witness is read from `h` again, so only a handle named by a local gets
  the exemption; a handle of a generic target whose parameter has no
  `Inspectable` bound names that parameter by its text;
- runtime type identity: importing a `std.inspect` name or `std.error.Error`
  declares the sealed `Inspectable` (`std.error.Error` extends it) and
  `TypeId`, a data type holding the canonical printable name (an inner
  `mut` kept, the outer `mut` dropped); every
  inspectable type erases to `Inspectable` or `mut Inspectable` through a
  generated dictionary whose `runtime_type` builds that name, splicing in the
  names carried by `T < Inspectable` dictionaries, or by the `Inspectable`
  part of a bound whose trait extends it, such as `E < Error`; `downcast`, `downcast_mut`,
  `downcast_val`, and `TypeId::of` are checker intrinsics that compare names
  and unwrap the stored payload; `impl Inspectable`, a redeclared or
  implemented `runtime_type`/`downcast`/`downcast_mut`, and an Inspectable
  requirement key in a function's requirement clause are rejected. Not
  covered: `Hash` for `TypeId`, Inspectable keys in
  closure types and provider scopes, qualified printable names (the
  prototype has one module), and opaqueness (`TypeId { key: ... }` is
  constructible). A type parameter instantiated with `mut U` looks up
  implementations for `U`, and its Inspectable dictionary adds the inner
  `mut` when a composite key is built from it;
- error derivation (spec/14-annotations.md#error-derivation), lowered before
  any decorator is resolved by `checker/error-derivation.ts`: it removes each
  `@error` form, and each `@from` and `@source` marker inside an error type,
  whatever a binding named `error`, `from`, or `source` means, and reports
  their placement (`decorator-target-kind`), arguments and cause members
  (`invalid-error-marker`), and overlaps (`overlapping-impl`).
  `checker/error-generation.ts` writes `impl Display`, `impl Error` with
  `cause`, and one `impl From[P]` per `@from` member as hd source with the
  generated bounds. Each message becomes a helper function whose parameters
  are the members it names, so `self` and unnamed shared data are unknown
  names there; a cause or transparent member goes through a bounded helper
  on a line with the member's span, so a member that is not an `Error` is
  `unsatisfied-trait-bound` there. Without `use std.error.Error` or
  `use std.convert.From`, the pass imports them under hidden names. The
  declared `std.error.Error` has `fn cause(self) -> Error?` with a `.None`
  default (spec/09-traits.md#r-trait.error.cause); an optional trait value
  type such as `Error?` is a known type, and a value of `T < Trait` erases
  to `Trait` through the bound's dictionary;
- executable `std.testing.assert` with source-order argument evaluation, plus
  `assert_equal` for supported scalar, string, tuple, list, optional, `Result`,
  and order-independent map values and for explicit nominal or bounded generic
  `Eq` implementations, with mandatory reasons and
  `missing-eq` at unsupported types. The checker checks the call and lowers
  it to a call of `check_equal`, hd code in `lib/std/testing.hd`, whose
  failure panics with `assertion-failed`, the reason, and both values'
  `debug` text through the `assertion_failed` host function
  (module.testing.assert-equal-debug); `std.testing.snapshot` runs as
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
  replayed before new cases on the next run. `examples` run first, one
  case each: the lowered test asks the `prop_example` host function which
  example to run, and the runner stops asking once the test reports no
  more. Each case has a draw budget of 256 draws (`prop_budget`); once
  `Choices` has spent it, every draw returns its simplest value without
  recording a draw. `Choices.int[N < Integer]` draws through `i64`, so a
  `u64` above the largest `i64` is never drawn;
- `@derive(Arbitrary)` and `impl Arbitrary for T by Structure` for
  `std.testing.Arbitrary` instantiate the template in `lib/std/testing.hd`;
  the checker generates nothing for `Arbitrary` itself.
  The derivation pass runs before std is joined, so it reads the template
  and its `impl Source for ArbitrarySource` from the std source, with the
  program's names (`standardTemplate` in `checker/standard-library.ts`);
  the std join drops both, since they name `std.structure`, which the
  pass declares only for a program that derives. A std module's `use`
  whose names only its template parts mention, such as `std.testing`'s
  `std.inspect` and `std.arbitrary` names, joins nothing by itself; a
  derivation that instantiates the template brings it in. The source picks the
  first variant whose `self_ref` is not `.Required` as the simplest, at
  drawn index 0, and fails with `NoFiniteValue` when no variant is finite,
  which the template reports as the panic `"${T::name()} has no finite
  value"`. For a generic target, each type parameter a member uses gets
  the template's trait and its source's `member` bound, read from
  `lib/std/testing.hd`, so `Box[T]` gets `T < Arbitrary & Inspectable`
  (`std-testing.arbitrary.derive.params`). Its
  `member[F < Arbitrary & Inspectable]` draws a member with
  the generator of its typed `With[F]` fact, read with `h.fact`, or else
  with `F::arbitrary`. A member that fails either bound
  is `unsatisfied-trait-bound` at the opt-in, naming the member, rather
  than the `member-not-derivable` of other templates
  (`std-testing.arbitrary.derive.not-derivable`). `use
  std.testing.arbitrary` makes `arbitrary.with` a call of
  `lib/std/arbitrary.hd`'s `with` (`checker/arbitrary-module.ts`);
- `Member.self_ref` and `VariantInfo.self_ref` (`SelfRef`) are computed in
  `checker/self-ref.ts`. It also follows owner decisions that agree with
  the spec text: any use of the enclosing declaration counts, whatever its
  type arguments; another enum needs the enclosing type when all its
  variants do; an omitted member counts;
- `--test-layout test-module|integration` compiles a file as a test module
  (the conformance Test Layouts); both layouts are test modules, since
  the prototype has no separate integration view;
- `--package-tree DIR --package-path PATH` (the conformance Package Trees)
  links FILE, as the package path PATH, with every `.hd` file under DIR,
  entered at FILE's module, and reports each diagnostic in the file it
  points into. Package dependencies (`package-cycle`) are not modeled;
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
  identity, and argument/result and provider configuration checks, through the
  in-process compiler API (the CLI has no record or replay command);
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
  assignment, `Index` and `IndexSet`, the callable-value traits `Apply`
  and `Update` (`v()` on a value that is not a function calls `apply`,
  with no argument, and `v() = x` calls `update` on a `mut` callee, a call
  of a declared function never being a place), supertrait bindings such as
  `Add[Self, Out = Self]`, and the sealed `std.num` traits `Num`, `Integer`,
  and `Float` follow the same path. The primitive implementations are hd
  code whose bodies are the built-in operators, and so are the index
  traits of `List`, `Map`, and `string`, whose bodies index directly.
  `m[k]` on a `Map` has type `V`, and a missing key panics with
  `index-out-of-bounds`; `m.get(k)` reads `V?`. Floating `%` calls the
  host's `rem_f64`, JavaScript's truncated remainder. Compound assignment
  `place op= value` stores `place op value` for every type, and an index
  place reads and stores its element. A newtype construction
  over an `AnyVal` base is readonly, and one over an `AnyRef` base carries
  its argument's permission, as unwrapping one does. `Num::from_i64` checks
  its range in `lib/std/num.hd`, and `"$x"` on `T < Num` reaches `Display`
  through the supertrait;
- homogeneous `List[T]` literals, indexing, `len()`, and mutable `append()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut List[T]`;
- insertion-ordered `Map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut Map[K, V]`, and erased Wasm GC key/value storage.
  A key type meets the declared bound `Map[K < Eq & Hash, V]`
  (types.map-key.declared-bound) through non-generic `Eq` and `Hash`
  implementations, std's `Hash` for the primitives included, or a type
  parameter's own bounds; a `mut` key type is `invalid-map-key`. The loader
  adds `Hash` for any code that mentions `Map` or writes a map literal or
  comprehension. An `i32`-like scalar or a string key compares directly; any
  other key compares through a wrapper of its type's `Eq`, or of the
  primitive `Eq` of `i64` and `u64`; and a map over a type-parameter key
  (key kind 3) compares its keys through the bound's `Eq` dictionary, which
  the map holds as its key context;
- the prelude `Iterator[T]`, a `lib/std/iter.hd` data type whose private
  `step` closure `next` calls, built by `Iterator::from_fn`, with the
  adapters `filter`, `take`, `enumerate`, `map`, `fold`, and `collect` as
  its ordinary methods (`checker/iteration.ts`). A list or map `iter()` is
  `from_fn` over a closure that advances the built-in Wasm GC cursor
  (`$Cursor[T]`, a name source code cannot spell), so explicit and
  `for`-loop iteration share exhaustion, partly consumed cursor,
  replacement, and structural invalidation behavior. A `for` loop over a
  list or map advances the cursor directly, and one over a `mut Iterator`
  calls its `next`. The loader declares `Iterator` when a program names it
  or `Iterable`, or selects `iter`, `take`, `enumerate`, `fold`, or
  `collect`. A private field of a std type is hidden from code outside std,
  which is the only field visibility the one-module prototype checks;
- `collect[C < FromIterator[T] = List[T]]` over the `std.iter` trait
  `FromIterator`, which is not a prelude name. `C` comes from an explicit
  type argument or the expected type, which reaches the operand of `x?` as
  `Result[T, E]` or `T?`, and otherwise from its ordinary type-argument
  default. `List`, `Map`, `Result`, and optional targets are hd code;
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
Strings and structural values in the host-provider ABI remain in later
MVP slices. `all!` calls are typed by their rule (each child a
`mut Suspend[X_i]`, the result `(X_1, ..., X_n)`), and `race!` calls by the
plain signature in `lib/std/task.hd`. Both drive one polling frame, a stored
suspension that `$hd.combinator` in `emitter/stored-suspension.ts`
implements: `race!` is hd code that drives the frame `race_frame` builds,
and the checker lowers each `all!` call to a drive of the frame
`all_frame` builds. Both builders are runtime primitives in
`lib/std/task.hd`. A `race!` with no tasks never completes.
There are no type packs: `...` in a type is only a rest element, and
`[Ts...]` is a `syntax-error`. GADT variant results are not implemented.
Interpolation and `println` report `unsatisfied-trait-bound` when the displayed type
does not implement the canonical prelude trait.

## Standard Library

The toy standard library is hd source in the top-level
[`lib/std/`](../lib/std/) directory, next to `src/` as in Zig, one file per
module: `std.annotation`, `std.cmp`, `std.collections`, `std.hash`, `std.console`, `std.format`, `std.function`, `std.iter`, `std.num`, `std.ops`,
`std.option`, `std.process`, `std.resource`, `std.result`, `std.testing`, `std.text`, and `std.time`. It
follows the specification's stdlib tier (`spec/std/`); open points are
in [Open Issues](../future-work/OPEN_ISSUES.md).
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
  implementation. One on a tuple, such as `Arbitrary` for `(A, B)`, also
  needs code that mentions a tuple type, expression, or pattern;
- a prelude name that std declares, such as `Eq`, `Display`, or `Console`,
  is added when the program, or a std declaration it gets, mentions it. A
  comparison operator or `assert_equal` mentions `Eq`, and `<` also
  `PartialOrd`; an interpolated string or a `to_string` call mentions
  `Display`. The checker finds them by name, which no program may
  shadow;
- every added declaration's span is the `use` that brought it in, or the
  program's span.

What it provides:

| Module | Contents |
| --- | --- |
| `std.annotation` | `facts_of`, with an `@intrinsic("facts_of")` body that never runs: the checker lowers each call to a builder that `checker/function-facts.ts` generates. `Target`, `Annotate`, and `annotate`, which limit a fact type's target kinds |
| `std.hash` | `Hash` and `Hasher` (prelude names), and `Hash` for `string`, `bool`, `char`, and every integer type, and its tuple template; no standard hasher, which the specification does not name |
| `std.task` | `retry!`, and `race!`, which drives the frame of the `@intrinsic("task_race_frame")` builder; `all!`'s frame builder, `@intrinsic("task_all_frame")`; `block_on`, `all!` (which has no written signature), and `Waker` stay compiler-provided names of the module |
| `std.option` | on `T?`: `map`, `unwrap_or`, `ok_or`, `is_some`, `is_none`, `expect` |
| `std.result` | on `Result[T, E]`: `map_ok`, `map_err`, `ok`, `err`, `is_ok`, `unwrap_or`, `expect` |
| `std.collections` | on `List[T]`: `map`, `filter`, `first`, `last`, `reversed`, `sorted_by` (stable), `chunks`, `zip` |
| `std.text` | on `string`: `chars`, `char_indices`, `bytes`, `slice`, `to_utf8`, `string::from_utf8` with `Utf8Error`, `is_empty`, `ends_with`, `contains`, `find`, `upper`, `trim_start`, `trim_end`, `strip_prefix`, `strip_suffix`, `lines`, `repeat`; `join`, `StringBuilder`; the prefix `r` and its helpers `interpolate`, `process_escapes`, and `EscapeError` |
| `std.iter` | the prelude `Iterator[T]` and `Iterable[T]`; `Iterator` with `from_fn`, `next`, and the adapters `filter`, `take`, `enumerate`, `map`, `fold`, and `collect`; `FromIterator` for `List`, `Map`, `Result`, and `T?`; `Iterable` for `List` and `Map` (not `Iterator`, which a loop advances directly); `range` |
| `std.cmp` | the prelude `Eq`, `PartialOrd`, `Ord`, and `Ordering`; `min`, `max`, `clamp`, `Reverse[T]`; `Eq` for `List`, `T?`, `Result`, and `Map`, and `PartialOrd` and `Ord` for `List` and `T?`; the tuple templates of `Eq`, `PartialOrd`, and `Ord` |
| `std.num` | the sealed `Num`, `Integer`, and `Float`, implemented for every primitive number type; on `i32` and `i64`: `checked_*`, `wrapping_add`, `wrapping_sub`, `saturating_*`, `abs_diff`, `count_ones`, `leading_zeros`; on `f64`: `is_nan`, `is_finite`; `parse_i32`, `parse_i64`, `ParseNumberError` |
| `std.time` | `Duration` with `milliseconds`, `seconds`, `as_milliseconds`; the suffix functions `ms`, `s`, `min`, `h` |
| `std.console` | the prelude `Console` and `println`; `ConsoleInput`, and the recording `BufferConsole` with `new` and `output` |
| `std.process` | `ExitCode`, `Termination`; `Process`, `Command`, `Output`, `ProcessError`, and the deterministic `ScriptedProcess` |
| `std.resource` | `ResourceError[E]` |
| `std.ops` | the twelve operator traits, `Index`, `IndexSet`, `Apply`, and `Update`, with the primitive implementations of the operator traits and the index traits' implementations for `List`, `Map`, and `string`; `NumSuffix` and `num_suffix`, the literal-suffix marker; `StrPrefix`, `str_prefix`, and `Template`; `Default` and its standard implementations, and its tuple template (spec/std/ops.md) |
| `std.function` | the sealed marker trait `Tuple`, which the compiler implements for every tuple type; a `Tuple` bound passes no dictionary. `Fn` and `SuspendFn` have no declaration: the checker rewrites them to the `fn(...)` sugar |
| `std.format` | the prelude `Display` and `Debug`; `DebugWriter` and the builders `DebugStruct`, `DebugTuple`, `DebugList`, `DebugMap`; the prelude `debug`; `Debug` for the primitives, `List`, `T?`, `Result`; the tuple templates of `Debug` and `Display` |
| `std.testing` | `Choices`, `Arbitrary` (for the primitives, `string`, `List`, `Map`, `T?`, `Result`, pairs, and triples), `snapshot_file`; the rest of `std.testing` is checked by the compiler |
| `std.testing.arbitrary` | `with` and the typed fact type `With[F]`, in `lib/std/arbitrary.hd`, since the prototype has no std submodules |

The prelude `string` methods live in `std.text` too, and `lower` and
`upper` are backed by the host. Positions and lengths are byte offsets. A string index is the
`string-index` HIR node, a bounds-checked byte read (`$hd.string_get`).
Prototype limits: `slice` copies its bytes instead of sharing them (the
runtime `string` is a bare `$hd.bytes` array, with no offset to share), no `parse_f64`, `wrapping_mul`, or `Float` rounding methods, no `Set` (the specification does not define it,
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
RUNTIME_AND_LIBRARY.md).

1. **Intrinsic functions.** A `lib/std` function preceded by
   `@intrinsic("name")` is an ordinary declaration whose body the compiler
   supplies. The standard-library loader turns the line into
   `FunctionDecl.intrinsic` (`checker/standard-library.ts`). The same line in
   user code is an ordinary decorator whose value calls an undeclared
   `intrinsic` (`unknown-name`), so only `lib/std` can use it. The written body (`panic("intrinsic")`)
   type-checks and is never emitted. Calls are ordinary calls.
   - A **runtime primitive** is a few Wasm instructions over the runtime's
     own value layout, listed in `emitter/intrinsics.ts`: today
     `string_byte_len`, `string_byte_at`, `string_byte_slice`,
     `char_from_scalar`, and `index_out_of_bounds`.
   - Every other name is a **host function**, imported as `hd`
     `host:<name>` through one generic path. Scalars cross as Wasm numbers,
     and a `string` crosses as a host handle that `emitter/runtime/boundary.wat`
     copies byte by byte. The host looks the name up in
     `src/host-functions.ts` (today `string_lower`, `string_upper`, and `string_from_scalar`), or
     in the runner's `hostFunctions` (`snapshot_file_check`, `src/snapshots.ts`).
2. **Host capability traits.** A capability is a trait with suspending
   methods (spec/11 and
   RUNTIME_AND_LIBRARY.md).
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
| Checker | `Console` trait declared in TypeScript (`program-types.ts`); `ConsoleError` as a primitive type name (`shared.ts`, `context.ts`, `termination.ts`) | std declarations | Done for `Console`, declared in `lib/std/console.hd`; `ConsoleError` remains, see Console below |
| Emitter | `emitConsole` (a hand-written host `Console` provider), `console.wat` (`$hd.console_print`) | capability | Done: the generic capability bridge, with `Result` results |
| Host glue | `console_byte` import | capability | Done: `Console.write_line` in `HOST_PROVIDERS`, left out of record and replay |
| Checker | `validateHostCapabilities` skipped `Console` | capability | Done: `Console` passes the same boundary check as any host capability |
| HIR | `assert` | `std.testing` | Remains: `assert` is checked by the compiler |
| HIR | `assert-equal` | `std.testing` | Done: the compiler checks an `assert_equal` or `snapshot` call and lowers it to a call of the hd `check_equal` |
| HIR | `snapshot-file` | `std.testing` | Done: `snapshot_file` is hd code in `lib/std/testing.hd` with a host function |
| HIR | `each-row-index`, `each-row-count`, `test-timeout` | test runner hooks | Remains: runner protocol, not library code |
| HIR | `debug-render` | `std.format` | Done: `debug`, `DebugWriter`, and its builders are hd code in `lib/std/format.hd` |
| HIR | `list-*`, `map-*`, `iterator-next` | built-in `List` and `Map` | Remains: the collection types are built into the runtime layout |
| HIR | `inspect-type-id`, `inspect-downcast` | `std.inspect` | Remains: runtime type identity is a compiler service |
| Checker | `block_on`, `all!`, `race!`, `facts_of`, `downcast_val` | spec-named intrinsics | Remains: the specification names them compiler intrinsics. `race!` is hd code in `lib/std` over the `task_race_frame` runtime primitive, and `facts_of` is declared there, so only its `@intrinsic` name is known; `all!` has no written signature, so the checker types it by name and lowers it to a drive of the `task_all_frame` primitive's frame. `facts_of` lowers to a call of a generated hd builder over `std.structure`'s `Facts`, with no HIR node |
| Checker | `Duration` for test `timeout`, `ExitCode` and `Termination` for entry results (`standard-traits.ts`, `termination.ts`) | `std.time`, `std.process` | Remains: language hooks that name a std type; the declarations are already hd |
| Checker | `Display`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Iterable`, `Any`, `Debug`, `Ordering` declared in TypeScript | prelude declarations | Done, except `Any` (`std.core`) and `Waker` (`std.task`), which have no `lib/std` file: the rest are hd in `std.cmp`, `std.format`, and `std.iter`, declared when a program mentions them (migration M2) |
| Emitter | `float.wat` and the `format_f64`, `format_f32`, `pow_f64`, and `rem_f64` imports | float display, `**`, and floating `%` | Remains: operator and interpolation support |

Counts: the HIR expression union had 92 kinds, of which 15 were library-
or capability-specific. The string step removed 5, the `println` step 1,
the `debug` and `snapshot_file` step 2, and the `assert_equal` step 1,
leaving 83 kinds, 6 of them specific: the test-runner hooks, `assert`,
and `std.inspect` rows above. No capability has a HIR node now.

### Console

`println` is hd code in `lib/std/console.hd`
([`module.prelude.println`](../spec/10-modules.md#r-module.prelude.println)).
It calls `write_line` without `!`, which makes a stored suspension, and
drives it with `std.task.block_on`, so it inherits all of `block_on`'s
rules with no checker case of its own
([MHP follow-ups](../future-work/OPEN_ISSUES.md#mutable-host-providers)).
Its `.Err` panic is an ordinary `panic` call. `block_on` is a
compiler-provided name that `lib/std/task.hd` does not declare: the loader
keeps a std module's `use std.task.block_on` line as a program `use` under
a hidden name, so the call is an ordinary `block_on` call. The loader adds
`println` under its own name when a program mentions it, because
`println` is a prelude name.

The compiled module is the entry module, so its top-level statements may
call `block_on` and `println`
([`req.drive.block-on.forbidden-contexts`](../spec/11-requirements-and-suspension.md#r-req.drive.block-on.forbidden-contexts)
forbids only non-entry module initialization). A linked package shares
one namespace, so the prototype cannot reject a driver in another
module's initialization.

The host console is a built-in entry of the generic capability bridge.
Its calls stay out of record and replay (`UNRECORDED_PROVIDERS`), as the
[Mutable Host Providers](../future-work/OPEN_ISSUES.md#mutable-host-providers)
decisions say, so a replay through the compiler API prints console lines
again rather than reading them back.

What remains:

1. `ConsoleError` stays a TypeScript type name until its variants and
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
- `cli.ts` implements the current command-line interface; `cli-args.ts`
  holds its command table, flag parsing, and help text; `cli-queries.ts`
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
- `../test/cli.test.ts` exercises the packaged CLI surface end to end,
  `../test/cli-commands.test.ts` its help output and flag errors, and
  `../test/agent-tooling.test.ts` the JSON diagnostics, `explain`, `def`, and
  `doc`.
