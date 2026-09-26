# Modules

Status: language specification draft.

Modules organize names by source path. Packages organize compilation,
dependencies, and Wasm artifacts.

## Package Manifest

An hd-lang package has an `hd.toml` manifest:

```toml
[package]
name = "my_app"
version = "0.1.0"

[source]
root = "src"

[dependencies]
billing = "1.2.0"
```

The default source root is `src`. The complete manifest schema, dependency
resolution algorithm, lockfile, and version semantics remain tooling work; the
language-level use model assumes that the manifest maps each dependency name
to one resolved package.

## Path-Inferred Modules

A source file's path relative to the source root determines its module name:

```text
src/user/types.hd    # user.types
src/user/service.hd  # user.service
```

There is no `module` or `package` declaration in source.

A directory is itself a module only when it contains `mod.hd`:

```text
src/user/mod.hd      # user
src/user/types.hd    # user.types
```

`mod.hd` is the directory module's source file and public index. Child modules
are not brought automatically into the parent, and parent declarations are not
implicitly visible in children.

Two files must not map to the same module identity. Case sensitivity of module
paths is defined by the source language rather than the host filesystem:

- every non-`mod.hd` path component used in a module identity must be an NFC
  Unicode identifier under the source identifier rules;
- module identities are case-sensitive; and
- a package is rejected if two source paths collide after full Unicode case
  folding followed by NFC normalization of each module path component, even
  when the host filesystem could otherwise distinguish them.

The final `.hd` suffix and the special filename `mod.hd` are lowercase and
case-sensitive. The compiler applies the declared Unicode data version from
the lexical rules to path validation and collision checks. This rejects
ambiguous module names before host filesystem case behavior can change the
module graph.

## Use Roots

Every absolute use path begins with one of these roots:

- `pkg` for the current package;
- `std` for the standard library;
- `dep.<name>` for a manifest dependency.

The package manifest distinguishes standard library, current package, and
external dependency namespaces; source syntax does not require a different
prefix for each dependency beyond `dep.<name>`.

Relative use paths use `self` and `super`:

```text
use self.types.{User, UserId}
use super.shared.{Email}
```

Relative lookup starts at the source file's containing directory module. For a
regular file `src/user/service.hd`, that base is `user`; for
`src/user/mod.hd`, the base is the `user` module itself; for a file directly
under `src`, the base is the package root namespace. `self` names that base and
each leading `super` moves to its parent.

Consequently, from `src/user/service.hd`, `self.types` resolves to
`pkg.user.types` and `super.shared` resolves to `pkg.shared`. Moving above the
package root is a compile-time error. Relative use paths cannot cross into `std`
or a dependency.

## Use Forms

Use a single public declaration or a module namespace:

```text
use std.time.Duration
use pkg.user.types
use pkg.user.types as user_types
```

Use selected public declarations:

```text
use pkg.user.types.{User, UserId}
use dep.billing.types.{UserId as BillingUserId}
```

Grouped uses may have a trailing comma. Wildcard uses are not supported. Enum
variants are members, not module declarations, and cannot be used directly.
Using a private or missing declaration is a compile-time error.

Only the grouped form accepts a `pub` prefix. Public facades expose selected
declarations rather than module namespace aliases.

Use declarations introduce names for the whole module and are resolved before
type checking.

## Prelude

Every module implicitly has the following public standard-library names in
scope. This implicit scope is called the prelude; it is equivalent to fixed
`use` declarations and does not create ambient host authority. A module
declaration, use, type parameter, parameter, or local binding may not shadow a
prelude name; every conflict is a `prelude-name-shadow` error. This includes a
redundant `use` that names the same declaration already supplied by the
prelude: prelude names are used directly and are not re-imported.

| Origin module | Implicit names |
| --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `char`, `string`, `void`, `list`, `map`, `Any`, `Reference`, `Result`, `Ok`, `Err`, `panic` |
| `std.format` | `Display` |
| `std.cmp` | `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Ordering` |
| `std.hash` | `Hash`, `Hasher` |
| `std.iter` | `Iterator`, `Iterable` |
| `std.console` | `Console`, `ConsoleError`, `println` |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` |
| `std.annotation` | `Annotation`, `Annotate`, `TypeAnnotator`, `DataAnnotator`, `EnumAnnotator`, `FuncAnnotator`, `FieldMetadata`, `VariantMetadata`, `ParamMetadata`, `AnnotationRef`, `ShapeMetadata`, `DeclarationId`, `DeclarationKind`, `PrimitiveKind`, `SourcePosition`, `TypeShape`, `DataShape`, `FieldShape`, `EnumShape`, `VariantShape`, `FnShape`, `ParamShape` |

The prelude functions have these signatures: `panic(message: string) ->
never` and `println[T < Display](value: T) -> void $ Console`. The standard
console surface includes:

```text
trait Console:
    fn write_line!(self, text: string) -> Result[void, ConsoleError]
```

`Console` is a host capability trait, and `ConsoleError` is its standard
boundary-safe error type; `ConsoleError` implements `Display`. Thus `println`
is convenient to name but not a global
host API: each call must be covered by a `Console` requirement row or a
lexical provider scope.

`Reference` is a sealed marker trait implemented by data values, stored enum
values, lists, maps, dynamic trait values, `Any`, closures, suspensions, and
runtime handles that have identity, and payload-free enum values with canonical
variant identity. It is not implemented by primitives, tuples, or optionals.
User code cannot implement it.

The following built-in methods are normative. Lengths and scalar positions use
`i32`.

| Receiver | Methods |
| --- | --- |
| `string` | `len(self) -> i32`; `trim(self) -> string`; `lower(self) -> string`; `split(self, separator: string) -> list[string]`; `replace(self, old: string, replacement: string) -> string`; `starts_with(self, prefix: string) -> bool` |
| `list[T]` | `len(self) -> i32`; `iter(self) -> mut Iterator[T]`; `map[U](self, transform: fn(T) -> U) -> list[U]` |
| `mut list[T]` | `append(mut self, value: T) -> void` plus the readonly methods |
| `map[K, V]` | `len(self) -> i32`; `get(self, key: K) -> V?` |
| `mut map[K, V]` | `remove(mut self, key: K) -> V?` plus the readonly methods |
| `T?` | `map[U](self, transform: fn(T) -> U) -> U?` |
| `Display` | `to_string(self) -> string` |

`list.map` and optional `map` are non-suspending and evaluate the transform in
source order. No `set` type is part of the core prelude.

String methods operate on Unicode scalar-value strings without locale. `lower`
uses Unicode Default Case Conversion with full mappings. `trim` removes the
Unicode `White_Space` property at both ends. `split(separator)` retains empty
pieces between adjacent separators and at either end; an empty separator
splits into one-scalar strings, with an empty input producing an empty list.
With a non-empty separator, an input without that separator, including the
empty string, yields one piece, so `"".split(",")` is `[""]`.
`replace` replaces non-overlapping matches from left to right, and an empty
`old` inserts the replacement at scalar boundaries. `starts_with` compares
scalar sequences exactly and performs no normalization or case folding.

## Standard Testing

`std.testing` exports these normative assertion functions:

```text
fn assert(condition: bool, reason: string) -> void
fn assert_equal[T < PartialEq](actual: T, expected: T, reason: string) -> void
```

`reason` is required and must explain the checked condition. A failed assertion
reports test failure when called from a test and otherwise causes an
`assertion-failed` runtime panic. `assert_equal` uses `PartialEq.eq`; it does not
grant implicit equality to its argument type.

## Module Initialization

A **script** is an entry module whose top-level executable statements are the
entry behavior and which has no `main` declaration. An **entry module** is the
selected root module of an executable package. If it contains both top-level
statements and `main`, its top-level statements initialize the module first and
then the runtime invokes `main`; that form is an executable entry module, not a
script. A **program instance** is one instantiated Wasm module graph together
with its module storage, provider bindings, and execution state.

Before execution, the compiler resolves the acyclic use graph reachable from
the selected script or executable entry module. Every reachable module is
initialized exactly once per program instance after all modules it uses have
been initialized. When multiple modules are otherwise ready, their fully
qualified module identities order them lexicographically, making initialization
independent of filesystem enumeration.

Within one module, named declarations are available before initialization and
top-level executable statements run in source order. Use declarations do not
execute as statements. Top-level bindings are initialized at their statement,
before later function bodies may access them. Their storage remains available
to functions in that module for the lifetime of the program instance. A module
with no top-level executable statements has no observable initialization step.
Before accepting a top-level executable statement, the compiler verifies that
every top-level binding in the transitive read set of each referenced function
or closure is already initialized. References passed as values and functions
reached by trait dispatch, interpolation, iteration, or another implicit call
are included. A trait method call through a generic bound or a dynamic trait
value reaches every implementation of that method in the module. This
definite-initialization check covers the whole module value-flow and call
graph. The check is local to one module. Note: an
implementation can compute it from a per-function summary of the top-level
bindings each function reads, combined bottom-up over the module's call graph;
it never needs another module's function bodies.

After dependency initialization, a script executes its top-level statements as
that module's initialization. An executable package then invokes `main` after
its entry module has initialized. Test runners initialize the test module and
the modules it uses before invoking discovered test blocks; test block bodies
are not part of module initialization.

Top-level code in a non-entry module must be requirement-free
initialization. It may not use `$.use`, enter a provider scope, make a bang
call, or call a callable whose requirement row is not empty. A script module may
use requirements and suspension only through an inferred entry requirement row,
which the compiler reports alongside `main!` rows for host configuration. A
script's top level is not itself a suspension driver; bang calls must occur in
a suspending entry function or another specified driver context.

This rule governs one program instance. Interactive cell re-execution and
durable replay have separate runtime histories described in
[`RUNTIME_AND_LIBRARY.md`](../future-work/RUNTIME_AND_LIBRARY.md).

## Public Uses And Visibility

Declarations are module-private by default. Prefix `pub` makes a declaration
available to other modules:

```text
pub type UserId(string)

pub data User:
    pub id: UserId
    pub email: string
```

Use `pub use` in a `mod.hd` file to expose a package-facing API:

```text
pub use pkg.user.types.{User, UserId}
pub use pkg.user.service.{load_user, save_user}
```

Another module can then use the names through the directory module:

```text
use pkg.user.{User, UserId, load_user}
```

A publicly used declaration must already be public in its defining module.
`pub use` introduces the same local binding as `use` and additionally exposes
that binding to other modules. It does not create a new declaration identity.
Cycles involving `use` or `pub use` are rejected.

There is no package-private visibility modifier: another module in the same
package can use only `pub` declarations. Fields and inherent methods are
module-private unless individually marked `pub`; enum variants inherit their
enum's visibility. Trait methods follow their trait's visibility; a usable
implementation additionally requires its target type to be visible.

A public declaration's complete source-level signature must not expose a
module-private declaration. This check recursively covers function parameters
and results, data fields, enum constructor data and payloads, alias/newtype
underlying types, trait bounds, supertraits, public generic arguments, and every
requirement-row key. A
private implementation detail may occur in a public function body but not in
its public typed interface.

## Name Resolution Across Packages

An absolute qualified name identifies one declaration after dependency
resolution. Two dependencies do not share declarations merely because their
package names and source paths match; resolved package identity is part of a
declaration's identity.

A package must not access another package except through declarations reachable
from that package's public module surface. Implementations may compile packages
separately.

Every public declaration is fully annotated: its complete signature, including
parameter and result types, requirement rows, suspension, generic parameters
with their bounds, variance, and reification, and the types of public fields
and enum data, is written in source. Nothing in a public signature is
inferred from a function body. Top-level bindings cannot be public.

A package interface must contain exported declaration identities and complete
signatures; visibility; generic kinds, variance, bounds, and reification;
requirement rows; associated types; every ordinary, local, and annotation
implementation head needed for coherence; and the bodies of pack and reified
code, which downstream compilation specializes. An interface may also carry
ordinary generic bodies to enable inlining, but downstream compilation must
not require them: an implementation compiles each ordinary generic function
in its defining package, and a downstream use supplies only its dictionaries.
A package interface is therefore determined by the package's declarations and
does not depend on any other function body. A downstream package can be
compiled as soon as the interfaces of its dependencies are known, without
waiting for their function bodies to be checked or compiled. Coherence is
checked at link time over the complete set of resolved interface files, so
linking may reject a graph even when each package compiled independently.

## Executable Entry Point

An executable entry point is a public top-level function named `main` or
`main!` with no parameters. It returns `void` or `Result[void, E]` with
`E < Display`, and it may declare a requirement row. Every key in that row must
be a host capability trait of the selected runtime profile; any other key is a
`nonhost-entry-requirement` error. A top-level `main` that is not public is an
ordinary function and is not an entry point.

The conventional non-suspending entry point is:

```text
pub fn main() -> void:
    ...
```

It may instead return `Result[void, E]`, in which case `Err` reports invocation
failure through the runtime adapter and requires `E < Display`; an error type
without that implementation is an `entry-error-not-display` error. The host
renders the error with `Display.to_string` and exits with status 1. A panic exits with a
distinct nonzero status selected by the runtime profile and poisons the program
instance. `main` has no source-level arguments;
process arguments, console access, environment, and other host facilities are
requirements supplied by the runtime through the requirement model.

A suspending entry point is spelled `main!`.

## Wasm Boundary

A **runtime profile** is a named compile-time set of host capability traits,
their boundary adapters, and runtime choices such as panic exit statuses. The
compiler receives the selected profile as build configuration. The default
profile contains at least the prelude `Console` trait; another profile may add
or omit host traits explicitly.

`pub` is module visibility, not Wasm export registration. A tool, workflow, or
other host-callable function becomes visible only through explicit registration
provided by its library or annotation facet.

Every registered boundary function is an entry point for provider checking.
Its registration contract selects a runtime profile and declares which
requirement traits that profile can bind, including application traits such as
`Database` when the adapter explicitly supports them. Every key in the
registered function's row must be in that bindable set, and the host must bind
all of them before invocation; otherwise registration or startup fails before
user code executes.

Registered boundaries initially allow recursively structural values:

- primitive scalar types and `string`;
- tuples;
- `list[T]` and `map[K, V]` whose contents are boundary-safe;
- data types and enums whose complete fields and payloads are boundary-safe;
- `T?` and `Result[T, E]` whose contained types are boundary-safe.

Mutable types, dynamic trait values, closures, and live runtime handles are not
boundary-safe. Requirement-row entries are host bindings and are not serialized
parameters.

Boundary values have tree semantics. Encoding a cycle is a boundary error;
when an acyclic graph shares a node, each incoming path encodes a duplicate
tree value and decoding does not restore sharing. Every field and enum payload
that crosses a boundary must be `pub`, so a host cannot construct private
state. Decoding a map invokes the key type's ordinary `Eq` and `Hash`
implementations. If either panics, the adapter reports
`boundary-decoder-panic`, does not enter the registered function, and treats the
event as an ordinary poisoning panic: the program instance must be discarded.

One program instance executes on one thread and has no shared-memory
parallelism or source-level atomics. Hosts may run multiple Wasm instances in
parallel only by exchanging boundary-safe values; their heaps, mutable globals,
suspension drivers, and annotation registries are disjoint.

The official compiler targets Wasm only. The official runtime uses Wasm GC for
managed language values and a WASI-compatible host boundary. Authority-bearing
providers originate at that boundary. Every host facility is injected through
an ordinary requirement trait; the eventual standard capability-trait set is
runtime and library work.

The exact component-model ABI and registration APIs are runtime and library
specification work.

Closable runtime handles use `std.resource.ResourceError[E]`:

```text
enum ResourceError[E]:
    Operation(error: E)
    Disposed
```

An operation whose ordinary error type is `E` returns
`Result[T, ResourceError[E]]`. Once the handle has been closed, every further
operation returns `Err(ResourceError.Disposed)` and must not trap or access the
host resource. Closing is itself an operation and a repeated close returns the
same `Disposed` error. This checked behavior applies through every alias.

## Tooling, ABI, And Unsupported Extensions

The complete `hd.toml` schema, lockfile, version constraints, and dependency
resolver belong to package tooling. The exact Wasm component boundary and
registration mechanism belong to the runtime ABI. hd-lang has no package-private
visibility or independent visibility for enum variants and trait methods.
