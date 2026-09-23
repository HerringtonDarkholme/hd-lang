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
language-level import model assumes that the manifest maps each dependency name
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
are not imported automatically into the parent, and parent declarations are not
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

## Import Roots

Every absolute import begins with one of these roots:

- `pkg` for the current package;
- `std` for the standard library;
- `dep.<name>` for a manifest dependency.

The package manifest distinguishes standard library, current package, and
external dependency namespaces; source syntax does not require a different
prefix for each dependency beyond `dep.<name>`.

Relative imports use `self` and `super`:

```text
import self.types.{User, UserId}
import super.shared.{Email}
```

Relative lookup starts at the source file's containing directory module. For a
regular file `src/user/service.hd`, that base is `user`; for
`src/user/mod.hd`, the base is the `user` module itself; for a file directly
under `src`, the base is the package root namespace. `self` names that base and
each leading `super` moves to its parent.

Consequently, from `src/user/service.hd`, `self.types` resolves to
`pkg.user.types` and `super.shared` resolves to `pkg.shared`. Moving above the
package root is a compile-time error. Relative imports cannot cross into `std`
or a dependency.

## Import Forms

Import a module namespace:

```text
import pkg.user.types
import pkg.user.types as user_types
```

Import selected public declarations:

```text
import pkg.user.types.{User, UserId}
import dep.billing.types.{UserId as BillingUserId}
```

Grouped imports may have a trailing comma. Wildcard imports are not supported.
Enum variants are members, not module declarations, and cannot be imported
directly. Importing a private or missing declaration is a compile-time error.

Imports introduce names for the whole module and are resolved before type
checking.

## Module Initialization

Before execution, the compiler resolves the acyclic import graph reachable from
the selected script or executable entry module. Every reachable module is
initialized exactly once per program instance after all modules it imports have
been initialized. When multiple modules are otherwise ready, their fully
qualified module identities order them lexicographically, making initialization
independent of filesystem enumeration.

Within one module, named declarations are available before initialization and
top-level executable statements run in source order. Imports and exports do not
execute as statements. Top-level bindings are initialized at their statement,
before later function bodies may access them. Their storage remains available
to functions in that module for the lifetime of the program instance. A module
with no top-level executable statements has no observable initialization step.

After dependency initialization, a script executes its top-level statements as
that module's initialization. An executable package then invokes `main` after
its entry module has initialized. Test runners initialize the test module and
its imports before invoking discovered test blocks; test block bodies are not
part of module initialization.

This rule governs one program instance. Interactive cell re-execution and
durable replay have separate runtime histories described in
[`RUNTIME_AND_LIBRARY.md`](../RUNTIME_AND_LIBRARY.md).

## Exports And Visibility

Declarations are module-private by default. Prefix `pub` makes a declaration
available for import:

```text
pub type UserId(string)

pub struct User:
    pub id: UserId
    pub email: string
```

Use grouped `export` in a `mod.hd` file to re-export a package-facing API:

```text
export pkg.user.types.{User, UserId}
export pkg.user.service.{load_user, save_user}
```

A re-exported declaration must already be public in its defining module. A
re-export does not create a new declaration identity. Import and re-export
cycles are rejected.

There is no package-private visibility modifier: another module in the same
package can import only `pub` declarations. Fields and inherent methods are
module-private unless individually marked `pub`; enum variants inherit their
enum's visibility. Trait methods follow their trait's visibility; a usable
implementation additionally requires its target type to be visible.

A public declaration's complete source-level signature must not expose a
module-private declaration. This check recursively covers function parameters
and results, struct fields, enum constructor data and payloads, alias/newtype
underlying types, trait bounds, supertraits, and public generic arguments. A
private implementation detail may occur in a public function body but not in
its exported typed interface.

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
- structs and enums whose complete fields and payloads are boundary-safe;
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
