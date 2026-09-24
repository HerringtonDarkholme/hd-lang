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
use std.cmp.PartialEq
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

## Module Initialization

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

After dependency initialization, a script executes its top-level statements as
that module's initialization. An executable package then invokes `main` after
its entry module has initialized. Test runners initialize the test module and
the modules it uses before invoking discovered test blocks; test block bodies
are not part of module initialization.

This rule governs one program instance. Interactive cell re-execution and
durable replay have separate runtime histories described in
[`RUNTIME_AND_LIBRARY.md`](../RUNTIME_AND_LIBRARY.md).

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
underlying types, trait bounds, supertraits, and public generic arguments. A
private implementation detail may occur in a public function body but not in
its public typed interface.

## Name Resolution Across Packages

An absolute qualified name identifies one declaration after dependency
resolution. Two dependencies do not share declarations merely because their
package names and source paths match; resolved package identity is part of a
declaration's identity.

A package must not access another package except through declarations reachable
from that package's public module surface. Implementations may compile packages
separately as long as exported type identities and signatures remain stable.

## Executable Entry Point

The conventional non-suspending entry point is:

```text
pub fn main() -> void:
    ...
```

It may instead return `Result[void, E]`, in which case `Err` reports invocation
failure through the runtime adapter. `main` has no source-level arguments;
process arguments, console access, environment, and other host facilities are
requirements supplied by the runtime through the requirement model.

A suspending entry point is spelled `main!`.

## Wasm Boundary

`pub` is module visibility, not Wasm export registration. A tool, workflow, or
other host-callable function becomes visible only through explicit registration
provided by its library or annotation facet.

Registered boundaries initially allow recursively structural values:

- primitive scalar types and `string`;
- tuples;
- `list[T]` and `map[K, V]` whose contents are boundary-safe;
- data types and enums whose complete fields and payloads are boundary-safe;
- `T?` and `Result[T, E]` whose contained types are boundary-safe.

Mutable types, dynamic trait values, closures, and live runtime handles are not
boundary-safe. Context requirements are host bindings and are not serialized
parameters.

The official compiler targets Wasm only. The official runtime uses Wasm GC for
managed language values and WASI-compatible host integration where applicable.
Every host facility is injected through an ordinary requirement trait; the
official runtime is expected to provide the standard capability traits.

The exact component-model ABI and registration APIs are runtime and library
specification work.

## Tooling, ABI, And Unsupported Extensions

The complete `hd.toml` schema, lockfile, version constraints, and dependency
resolver belong to package tooling. The exact Wasm component boundary and
registration mechanism belong to the runtime ABI. hd-lang has no package-private
visibility or independent visibility for enum variants and trait methods.
