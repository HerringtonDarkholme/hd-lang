# Modules

Status: language specification draft.

This chapter defines modules, packages, the prelude, program initialization,
entry points, and the Wasm boundary.

1. r[module.kind.module] Modules organize names by source path.
2. r[module.kind.package] Packages organize compilation, dependencies, and Wasm artifacts.

## Package Manifest

An hd-lang package has an `hd.toml` manifest:

```toml
[package]
name = "my_app"

[source]
root = "src"

[dependencies]
billing = "github.com/acme/billing@1.2.0"
```

1. r[module.manifest.file] An hd-lang package has an `hd.toml` manifest.
2. r[module.manifest.source-root] The default source root is `src`.
3. r[module.manifest.dependency] The language-level use model assumes that the manifest maps each dependency name to one resolved package.
4. r[module.manifest.no-registry] There is no package registry. A dependency is fetched from the version control repository that its host path names.
5. r[module.manifest.targets] A package may have a library, executables, or both.
6. r[module.manifest.no-features] A manifest declares no optional features, and source has no conditional compilation.

> **Why.** Source never names where a dependency comes from, so moving to
> version control hosts changes no `use` line. One build per package
> version keeps one interface per version.

See also: [Tooling, ABI, And Unsupported Extensions](#tooling-abi-and-unsupported-extensions),
[Command Line](../cli/command-line.md) for executables, tasks, and package mode.

### Dependency Requirements

A **dependency requirement** maps a dependency key to a host path and a
minimum version:

```toml
[dependencies]
json = "github.com/acme/json@2.1.0"
json_v1 = "github.com/acme/json@1.9.0"
lint = "github.com/acme/tools/lint@2.3.0"
billing = "git.example.com/shop/billing.git@0.4.2"
pdf = "github.com/acme/pdf@0.4.1-0.20260912081500-3f2c9e1a7b6d"
```

1. r[module.dep.key-name] Each key of `[dependencies]` and `[dev-dependencies]` names one dependency, which source writes as `dep.NAME`. `NAME` is the key with each `-` replaced by `_`, so the key `my-app` is `dep.my_app`.
2. r[module.dep.key-name.collision] Two keys of one manifest whose `NAME`s are equal, such as `my-app` and `my_app`, are invalid.
3. r[module.dep.requirement-form] Each value is a dependency requirement `PATH@VERSION`, or a [path requirement](#r-module.path-dep.form) on a local package. A dependency requirement is a host path, `@`, and a version or pseudo-version without a leading `v`.
4. r[module.dep.path-manifest-only] A host path appears only in the manifest. Source names a dependency only through its key.
5. r[module.dep.identity] A resolved package's identity is its host path and its [compatibility line](#r-module.version.line).
6. r[module.dep.no-self-path] A manifest does not state its own host path. A fetched package's host path is the one that the dependency requirement which fetched it names.
7. r[module.dep.two-lines] Two compatibility lines of one host path are two dependencies with two keys, as `json` and `json_v1` above.
8. r[module.dep.one-key-per-line] Two keys of one manifest that name the same host path and compatibility line are invalid, since one package would then have two names.
9. r[module.dep.no-major-suffix] A host path carries no major-version suffix such as `/v2`.

> **Why.** Go puts `/v2` in the path because its imports repeat the path.
> In hd, source says `dep.json`, so a second key tells two lines apart, and
> a major upgrade edits one manifest line.

### Host Paths

A host path names a repository, and optionally a package directory inside
it:

| Host | Repository part | Example |
| --- | --- | --- |
| `github.com` | the host, an owner, and a repository segment | `github.com/acme/json` |
| any other host | the path up to a segment that ends in `.git` | `git.example.com/shop/billing.git` |

1. r[module.repo.github] `github.com` is the one known host. On it, the first three segments, the host, an owner, and a repository, name the repository.
2. r[module.repo.git-suffix] On any other host, the repository part must end with a segment that ends in `.git`.
3. r[module.repo.no-discovery] The toolchain never fetches a web page to discover a repository, as Go's `go-import` meta tag does.
4. r[module.repo.subdirectory] Segments after the repository part name the directory that holds the package. One repository may hold several packages this way.
5. r[module.repo.credentials] A private repository is fetched with git's own credentials, such as its credential helpers and SSH keys.
6. r[module.repo.no-stored-credentials] The toolchain stores no credentials.
7. r[module.repo.private-pattern] A private-path pattern marks host paths as private. The toolchain never sends a private path to a checksum log or proxy.

### Versions

A package's versions are the git tags of its repository:

| Package | Tag | Version in a dependency requirement |
| --- | --- | --- |
| at the repository root | `v2.3.0` | `2.3.0` |
| in the subdirectory `lint` | `lint/v2.3.0` | `2.3.0` |
| an untagged commit after `v0.4.0` | none | `0.4.1-0.20260912081500-3f2c9e1a7b6d` |

1. r[module.version.tag] A version is a git tag `vMAJOR.MINOR.PATCH`, with an optional SemVer 2.0.0 pre-release suffix.
2. r[module.version.tag-prefix] A package in a subdirectory uses tags prefixed with that subdirectory's path and `/`, such as `lint/v2.3.0`.
3. r[module.version.tag-only] The tag is the only version. A manifest does not state its own package's version.
4. r[module.version.pseudo] A **pseudo-version** names one untagged commit. Its form depends on the closest earlier tag of the package, as the table below shows.
5. r[module.version.pseudo.commit] In a pseudo-version, `TIME` is the commit's UTC time as `yyyymmddhhmmss`, and `HASH` is the first 12 hexadecimal digits of the commit hash.
6. r[module.version.order] Versions, pseudo-versions included, are ordered by SemVer 2.0.0 precedence.
7. r[module.version.line] The **compatibility line** of a version is its major number when the major is at least 1, and `0.MINOR` when the major is 0.
8. r[module.version.pseudo.release] A tagged version's manifest may require a pseudo-version.
9. r[module.version.no-bare-path-release] A tagged version's manifest must not hold a path requirement that carries no version. Such a version is rejected when it is fetched, and the toolchain does not tag one.
10. r[module.version.tag-missing] A dependency requirement whose version is not a pseudo-version, and whose package has no tag for that version, is invalid. So `lint = "github.com/acme/tools/lint@2.4.1"` is invalid when the repository has no tag `lint/v2.4.1`.
11. r[module.version.no-fallback] The toolchain never falls back to an untagged commit or to a nearby version in place of a missing tag.
12. r[module.version.pseudo-missing] A pseudo-version whose `HASH` names no commit of the package's repository, or whose `TIME` is not that commit's time, is invalid, as a missing tag is.
13. r[module.version.pseudo-missing.no-fallback] The toolchain never falls back to another commit or version in place of such a pseudo-version.

Pseudo-versions take Go's three forms:

| Closest earlier tag | Pseudo-version |
| --- | --- |
| none | `0.0.0-TIME-HASH` |
| a release `vX.Y.Z` | `X.Y.(Z+1)-0.TIME-HASH` |
| a pre-release `vX.Y.Z-PRE` | `X.Y.Z-PRE.0.TIME-HASH` |

> **Note.** A pseudo-version is a pre-release, so it orders below the
> release it precedes, and pseudo-versions of one base order by time.

> **Why.** A requirement names one exact release, so a typo or a deleted
> tag fails the build rather than silently choosing other code. Go reports
> the same case as `unknown revision`, and rejects a pseudo-version that
> does not match its commit. The error is `unknown-version`, by
> [`cli.dep.unknown-version`](../cli/command-line.md#r-cli.dep.unknown-version).

### Version Selection

Selection is **minimal version selection**: each dependency requirement is
a minimum, and the build uses the largest minimum stated for each package:

| Manifest | Requires |
| --- | --- |
| the root package | `json@2.1.0`, `billing@1.4.2` |
| `billing` 1.4.2 | `json@2.3.0` |
| selected | `json` 2.3.0, `billing` 1.4.2 |

1. r[module.select.minimum] A dependency requirement states a minimum. Its version, or any later version in the same compatibility line, satisfies it.
2. r[module.select.reach] Selection starts at the root package, or at every member of a workspace, and reads the manifest of each version that a dependency requirement reaches.
3. r[module.select.dev-dependencies] Selection reads the dev dependencies of the root package or workspace members only. A dependency's dev dependencies are never read.
4. r[module.select.largest] For each host path and compatibility line, the selected version is the largest minimum that any reached manifest states.
5. r[module.select.one-per-line] A package graph therefore holds at most one version per compatibility line. Two lines of one host path may coexist as two packages.
6. r[module.select.no-lock] The manifests alone determine the selection. There is no lockfile of versions.
7. r[module.select.path] Selection also reads the manifest of each package that a [path requirement](#r-module.path-dep.form) reaches, from the root package, a member, or another such package. Its dependency requirements join the selection.

> **Note.** A package that a path requirement reaches is a dependency, so
> its dev dependencies are never read, by
> [`module.select.dev-dependencies`](#r-module.select.dev-dependencies),
> unless it is also a member of the workspace.

> **Why.** Selection reads only the manifests of versions someone names,
> which needs no registry index. A new tag reaches no build until some
> manifest names it.

### Integrity

A committed `hd.sum` file is what makes a fetched dependency trusted.

1. r[module.sum.file] A root package or workspace has an `hd.sum` file beside its manifest. It records a hash of the source tree of each selected version.
2. r[module.sum.committed] `hd.sum` is kept under version control with the manifest.
3. r[module.sum.mismatch] A fetched tree whose hash differs from its `hd.sum` entry is rejected. It is never only a warning.
4. r[module.sum.only] `hd.sum` is the only integrity source. A build must not require a checksum log, a proxy, or any service besides the repository hosts.
5. r[module.sum.path] `hd.sum` holds no line for a package that a path requirement reaches, neither for its tree nor for its manifest. Its files are local and are never fetched.

> **Why.** hd runs no paid servers. A checksum log or a caching proxy may
> be added later only if it needs no infrastructure, or reuses free public
> infrastructure.

### Workspaces

A workspace builds several packages of one repository as one graph.

1. r[module.workspace.definition] A **workspace** is a set of packages that one workspace manifest, an `hd.toml` at the workspace root, lists as members.
2. r[module.workspace.committed] The workspace manifest may be kept under version control, so every checkout builds the same graph.
3. r[module.workspace.selection] Selection runs once for the whole workspace, so all members use the same selected versions.
4. r[module.workspace.sum] A workspace has one `hd.sum`, beside its workspace manifest.
5. r[module.workspace.no-host-path] A member does not require another member by host path.
6. r[module.workspace.fetched-member] A fetched package may require a member's host path. Selection then treats that host path as any other and fetches the selected version, which is a package separate from the local member.
7. r[module.workspace.fetched-member.two] The build then holds two packages, the local member and the fetched version. Neither stands in for the other.

> **Note.** A workspace is optional. It shares one selection and one
> `hd.sum` among its members; a [path requirement](#path-requirements)
> works with or without one.

> **Why.** A member states no host path
> ([`module.dep.no-self-path`](#r-module.dep.no-self-path)), so nothing
> ties it to a fetched version of the same repository. Cargo likewise
> treats a path source and a git source of one crate as two packages.

### Path Requirements

A path requirement names a local package by its directory, in a workspace
or outside one:

```toml
[dependencies]
billing = { path = "../billing" }
ui = { path = "../ui", version = "0.4.2" }
json = "github.com/acme/json@2.1.0"
```

1. r[module.path-dep.form] A **path requirement**, `{ path = "DIR" }`, names the package in the directory `DIR`, relative to the requiring manifest's directory.
2. r[module.path-dep.any-package] `DIR` may hold any local package. It need not be a member of the requiring package's workspace, and neither package needs a workspace.
3. r[module.path-dep.no-package] A path requirement whose `DIR` holds no manifest that declares a package is invalid.
4. r[module.path-dep.root-selection] The selection and `hd.sum` of the root package, or of its workspace, hold for every package that a path requirement reaches. That package's own `hd.sum` and workspace are not read.
5. r[module.workspace.path-version] A path requirement may also carry a version, as in `{ path = "../ui", version = "0.4.2" }`.
6. r[module.workspace.path-version.locally] When the requiring package is built from local files, in its workspace or in a checkout of its repository, such a requirement names the package at its path, and its version is not used.
7. r[module.workspace.path-version.fetched-version] In a fetched version of the requiring package, `path` is ignored, and the requirement is a dependency requirement on `version`.
8. r[module.workspace.path-version.fetched-host] Its host path comes from the host path by which the requiring package was fetched, as [`module.dep.no-self-path`](#r-module.dep.no-self-path) gives every fetched package its host path.
9. r[module.path-dep.fetched-outside] In a fetched version, a path requirement whose `DIR` lies outside the repository is invalid, since no host path names that directory.

> **Note.** A package whose path requirements all carry a version is
> released without a manifest edit, by
> [`module.version.no-bare-path-release`](#r-module.version.no-bare-path-release),
> as Cargo's
> [multiple locations](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations)
> allow. A package with a bare path requirement must name the other package
> by dependency requirement in its release manifest.

> **Why.** A path requirement is for local development, as Cargo's
> [path dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-path-dependencies)
> are. Cargo records no checksum for a path dependency, and builds it with
> the root's lockfile, never its own. A workspace only adds a shared
> selection and `hd.sum`.

### Toolchain Version

A manifest states which toolchain versions can build its package.

1. r[module.toolchain.minimum] A manifest may state a minimum toolchain version, which is also its minimum `std` version.
2. r[module.toolchain.graph-minimum] A build whose toolchain is older than the minimum of any package in the selected graph is rejected.
3. r[module.toolchain.pin] Only a root manifest may pin one exact toolchain version.
4. r[module.toolchain.no-editions] There are no language editions.

## Path-Inferred Modules

A source file's path relative to the source root determines its module name:

```text
src/user/types.hd    # user.types
src/user/service.hd  # user.service
```

1. r[module.path.name] A source file's path relative to the source root determines its module name.
2. r[module.path.no-declaration] There is no `module` or `package` declaration in source.

### Directory Modules

A directory is itself a module only when it contains `mod.hd`:

```text
src/user/mod.hd      # user
src/user/types.hd    # user.types
```

1. r[module.path.directory] A directory is itself a module only when it contains `mod.hd`.
2. r[module.path.mod-file] `mod.hd` is the directory module's source file and public index.
3. r[module.path.no-child-import] Child modules are not brought automatically into the parent.
4. r[module.path.no-parent-scope] Parent declarations are not implicitly visible in children.
5. r[module.path.no-std-child-import] `std` follows the same rule: after `use std.testing`, the path `testing.arbitrary.x` is an error. A std module that wants to expose a child module's items re-exports them with `pub use`.

```text
use std.testing

fn make(x: testing.arbitrary.With) -> i32:  # unknown-type: no child import
    0
```

### Root Files

Two files directly under the source root have fixed roles:

| File | Role |
| --- | --- |
| `src/lib.hd` | the package root module, which `pkg` names |
| `src/main.hd` | the [default executable](../cli/command-line.md#r-cli.exe.default-main)'s entry, its own program |

1. r[module.path.lib-file] `lib.hd` directly under the source root, `src/lib.hd`, is the package root module. Its public declarations are what `pkg.{X}` names, and what a dependent names through its dependency key.
2. r[module.path.lib-index] `src/lib.hd` is the root's public index, as a directory's `mod.hd` is the directory's.
3. r[module.path.main-file] `main.hd` directly under the source root, `src/main.hd`, is an entry module and its own program, never part of the package's library.
4. r[module.path.main-no-use] A use of `src/main.hd` from another module is an error. Error: `unknown-module`.
5. r[module.path.lib-only-root] `src/lib.hd` is the only root file of a package's library; the source root itself is never a directory module.
6. r[module.path.no-root-mod] A `mod.hd` directly under the source root, `src/mod.hd`, is an error, and its message suggests renaming it `src/lib.hd`.
7. r[module.path.lib-target] A package has a library exactly when it has `src/lib.hd`.
8. r[module.path.executable-only] A package without `src/lib.hd` is executable-only: no dependent can use its modules.
9. r[module.path.no-lib-dependency] Depending on a package that has no library is an error.
10. r[module.path.reserved-pkg] A module named `pkg` directly under the source root, as `src/pkg.hd` or `src/pkg/mod.hd`, is an error. `pkg` names the root module. Error: `reserved-module-name`.

```text
src/lib.hd     # the package root module
src/mod.hd     # invalid: rename it src/lib.hd
src/pkg.hd     # invalid: pkg names the package root module
```

> **Note.** `src/main.hd` reaches the library's declarations through
> `pkg` or `self`, as in `use self.{Config}`.

> **Why.** One root file per role, as in Cargo: `src/lib.rs` makes the
> library, and `src/main.rs` the default binary. `pkg` is reserved as
> Rust reserves `crate`, so a module never shares the root's name.

### Module Identity

1. r[module.path.unique] Two files must not map to the same module identity.
2. r[module.path.case-source] Case sensitivity of module paths is defined by the source language rather than the host filesystem, by the rules below.
3. r[module.path.identifier] Every non-`mod.hd` path component used in a module identity must be an NFC Unicode identifier under the source identifier rules.
4. r[module.path.case-sensitive] Module identities are case-sensitive.
5. r[module.path.case-collision] A package is rejected if two source paths collide after full Unicode case folding followed by NFC normalization of each module path component. This holds even when the host filesystem could otherwise distinguish them.
6. r[module.path.suffix] The final `.hd` suffix and the special filename `mod.hd` are lowercase and case-sensitive.
7. r[module.path.unicode-version] The compiler applies the declared Unicode data version from the lexical rules to path validation and collision checks.

> **Why.** Checking paths this way rejects ambiguous module names before
> host filesystem case behavior can change the module graph.

### Test Modules

A file whose name ends in `_test.hd` is a test module, and the test root
holds integration tests:

```text
src/billing.hd         # billing
src/billing_test.hd    # billing_test, a test module
tests/checkout.hd      # tests.checkout, an integration test program
tests/common/mod.hd    # tests.common, shared by integration test programs
```

1. r[module.test.module] A source file whose name ends in `_test.hd` is a **test module**, such as `src/billing_test.hd`, whose module is `billing_test`.
2. r[module.test.integration] An **integration test module** is a module under the package's test root, which is `tests` by default.
3. r[module.test.code] **Test code** is a package's `tests:` blocks, test modules, and integration test modules. Only a test build, such as `hd test` makes, compiles it.
4. r[module.test.code.doc] The package's [doc tests](#doc-tests) are test code too.
5. r[module.test.module.view] A test module is otherwise an ordinary module of its package: it sees public declarations package-wide and may use other test modules.
6. r[module.test.module.import] Seeing a public declaration means a test module may import it: the declaration is in its scope only through a `use`, as for any module.
6. r[module.test.integration.view] An integration test module sees the package as a dependent package does: its public declarations, built without its test code.
7. r[module.test.integration.program] Each file directly under the test root, such as `tests/checkout.hd`, is an **integration test program**: its own program, compiled separately from the others.
8. r[module.test.integration.shared] A module in a subdirectory of the test root, such as `tests/common/mod.hd`, is a **shared test module**. Every integration test program of the package may use it.
9. r[module.test.integration.beside-dir] A file directly under the test root beside a directory of the same name, such as `tests/common.hd` beside `tests/common/`, is invalid. Its fix-it moves the file to `tests/common/mod.hd`.
10. r[module.test.integration.program-use] A use of an integration test program from another module is an error. Error: `unknown-module`.
11. r[module.test.integration.shared-copy] Each integration test program gets its own copy of the shared test modules it uses, so their top-level statements run once per program.
12. r[module.test.integration.shared-unused] A warning that a declaration of a shared test module is unused is given only when no integration test program uses that declaration.
13. r[module.test.integration.pkg-root] In an integration test module, the `pkg` root names the package's library modules, each with only its public declarations.
14. r[module.test.integration.self-shared] An integration test program uses a shared test module through `self`, as in `use self.common` for `tests/common/mod.hd`.
15. r[module.test.no-tests-root] There is no `tests` use root: test code reaches the test root only through `self` and `super`.
16. r[module.test.no-tests-block] A test module or an integration test module must not contain a `tests:` block. Error: `misplaced-tests-block`.
17. r[module.test.dev-dependency] A **dev dependency** is a dependency that the manifest declares in `[dev-dependencies]`. Test code may use it, and a dependent package never sees it.
18. r[module.test.non-test-use.test-module] Non-test code that uses a test module is an error. Error: `test-only-use`.
19. r[module.test.non-test-use.dev-dependency] Code under the source root, other than test code, that uses a dev dependency is an error. Error: `test-only-use`.
20. r[module.test.cyclic-dev-unit] A dev dependency that itself depends on the package must not be used from a `tests:` block or a test module. Error: `cyclic-test-dependency`.
21. r[module.test.cyclic-dev-allowed] Integration test modules and [tasks](../cli/command-line.md#tasks) may use such a dev dependency.

> **Why.** Each integration test program builds on its own, as each Cargo
> integration test is its own crate, so helpers go in a subdirectory such
> as `tests/common/`. A dev dependency that depends back would give a unit
> test a second copy of the package, whose types differ from the ones
> under test. An integration test sees only the one normal build. A test
> module is already test code throughout, so it holds its test cases at
> top level rather than in a `tests:` block.

> **Note.** A [task](../cli/command-line.md#tasks) may also use dev
> dependencies, and the `tasks` directory shares modules as the test root
> does, through `self`.

See also: [Test Blocks](02-grammar.md#test-blocks),
[Standard Testing](#standard-testing).

#### Doc Tests

A fenced `hd` block in a documentation comment is a test of its own, which
`hd test` compiles and runs:

```text
# src/text.hd
## Turns a title into a URL slug: each space becomes a hyphen.
##
## ```hd
## use pkg.text.{slugify}
## use std.testing.assert_equal
##
## assert_equal(slugify("Ship It Now"), "Ship-It-Now", reason="each space becomes a hyphen")
## ```
pub fn slugify(title: string) -> string:
    let slug = ""
    for letter in title.chars():
        if letter == ' ':
            slug = "${slug}-"
        else:
            slug = "${slug}${letter}"
    slug
```

1. r[module.test.doc.block] Each fenced code block whose info string is `hd`, inside a [documentation comment](01-lexical-structure.md#documentation-comments) of a module under the source root, is a **doc test**.
2. r[module.test.doc.fence] A fence with any other info string, such as `text`, is not a doc test, and no build compiles its contents.
3. r[module.test.doc.program] Each doc test compiles as its own program, separately from the package's other test code, and holds exactly one test case.
4. r[module.test.doc.uses] A doc test's `use` declarations must come first in its block, and they are the uses of its program.
5. r[module.test.doc.body] The rest of the block is the body of that test case, as a trailing block given to `it` is.
6. r[module.test.doc.view] A doc test sees the package as an integration test module does, by [`module.test.integration.view`](#r-module.test.integration.view) and [`module.test.integration.pkg-root`](#r-module.test.integration.pkg-root).
7. r[module.test.doc.relative] A use path in a doc test that starts with `self` or `super` is an error. Error: `unknown-module`.
8. r[module.test.doc.not-integration] A doc test is not an integration test module, and a rule for those applies to it only where this section cites the rule.
9. r[module.test.doc.requirements] The runner binds a doc test's requirements by [`module.testing.integration-row`](#r-module.testing.integration-row) and [`module.testing.integration-row.unbound`](#r-module.testing.integration-row.unbound), as for an integration test case.
10. r[module.test.doc.private-items] The documentation comment of a private declaration or member holds doc tests too, and they still see only the package's public declarations.
11. r[module.test.doc.compile-fail] A doc test that holds a line comment `# error: CODE` is a compile-fail doc test. It never runs, and it passes only when compiling it reports `CODE`.
12. r[module.test.doc.compile-fail.fails] A compile-fail doc test whose compilation reports no diagnostic with that code fails, including when it compiles.
13. r[module.test.doc.no-attributes] A doc test has no other attributes: no hidden lines, no ignore option, and no comparison of its output.

A compile-fail doc test shows a misuse that the compiler rejects:

```text
# src/slug.hd
## A URL slug. Its text is private, so every slug comes from `make_slug`.
##
## ```hd
## use pkg.slug.{make_slug}
##
## slug := make_slug("Ship It")
## _ := slug.text  # error: private-member
## ```
pub data Slug:
    text: string

pub fn make_slug(title: string) -> Slug:
    let text = ""
    for letter in title.chars():
        if letter == ' ':
            text = "${text}-"
        else:
            text = "${text}${letter}"
    Slug { text: text }
```

> **Note.** A doc test is test code, so it may use the package's dev
> dependencies, by [`module.test.dev-dependency`](#r-module.test.dev-dependency).
> It checks output with `snapshot`, as in
> `snapshot(slugify("Ship It"), expect="Ship-It")`.

> **Note.** `hd_run!` runs only in an integration test module
> ([`std-testing.hd-run.integration-only`](../std/testing.md#r-std-testing.hd-run.integration-only)),
> so a doc test that calls it is a `test-only-use` error.

> **Why.** A doc test checks an example as a reader would copy it, so it
> sees only what a dependent sees. Blocks on private items run too, as
> rustdoc runs them, so internal docs stay true. `snapshot` already
> compares output, so a block needs no output attribute.

See also: [Test Runs](../cli/command-line.md#test-runs), for how `hd test`
names, filters, and reports doc tests.

## Use Roots

Every absolute use path begins with one of these roots:

| Root | Names |
| --- | --- |
| `pkg` | the current package |
| `std` | the standard library |
| `dep.<name>` | a manifest dependency |

1. r[module.root.absolute] Every absolute use path begins with one of the roots in the table.
2. r[module.root.manifest] The package manifest distinguishes standard library, current package, and external dependency namespaces.
3. r[module.root.dep-prefix] Source syntax does not require a different prefix for each dependency beyond `dep.<name>`.
4. r[module.root.use-only] Only a use declaration may begin a path with a root. Only a use brings a module name into scope.
5. r[module.root.use-only.error] So an absolute path in an expression or a type, such as `std.text.join(words, " ")` with no `use std.text`, is an error. Error: `unknown-name`.

```text
fn headline(words: List[string]) -> string:
    std.text.join(words, " ")  # error: unknown-name
```

### Relative Uses

Relative use paths use `self` and `super`:

```text
# src/user/service.hd
use self.types.{Query}          # src/user/service/types.hd
use super.types.{User, UserId}  # src/user/types.hd
```

Relative lookup starts at the source file's own module, which `self`
names. A root file starts at its root instead:

| Source file | `self` | `super` |
| --- | --- | --- |
| `src/a.hd` | `a` | the package root namespace |
| `src/user/service.hd` | `user.service` | `user` |
| `src/user/mod.hd` | the `user` module itself | the package root namespace |
| `src/lib.hd`, `src/main.hd` | the package root namespace | an error |
| `tests/checkout.hd` | the test root | an error |

1. r[module.relative.keywords] Relative use paths use `self` and `super`.
2. r[module.relative.base.current] Except in a root file, relative lookup starts at the source file's own module, as the table shows. In `mod.hd`, that module is the directory module.
3. r[module.relative.root-file] A **root file** is `src/lib.hd`, `src/main.hd`, or an [integration test program](#r-module.test.integration.program). Relative lookup in a root file starts at its root: the package root for a file under `src`, and the test root for one under `tests`.
4. r[module.relative.self.current] `self` names the module where relative lookup starts, so `self.x` names its child module `x`.
5. r[module.relative.super] Each leading `super` moves to its parent.
6. r[module.relative.root-file.super] A `super` in a root file is an error. Error: `unknown-module`.
7. r[module.relative.example.nested] Consequently, from `src/user/service.hd`, `self.types` resolves to `pkg.user.service.types`, in `src/user/service/types.hd`, and `super.types` resolves to `pkg.user.types`, in `src/user/types.hd`.
8. r[module.relative.example.top-level] From `src/a.hd`, `self.x` resolves to `pkg.a.x`, in `src/a/x.hd`. A sibling `src/b.hd` is `super.b`, since `super` from a file directly under `src`, other than a root file, is the package root.
9. r[module.relative.example.mod-file] From `src/a/mod.hd`, `self.x` resolves to `pkg.a.x`, in `src/a/x.hd`, since `self` there is the directory module.
10. r[module.relative.example.main] From `src/main.hd` or `src/lib.hd`, `self.x` resolves to `pkg.x`, in `src/x.hd`.
11. r[module.relative.above-root] Moving above the package root is a compile-time error.
12. r[module.relative.no-cross] Relative use paths cannot cross into `std` or a dependency.
13. r[module.relative.test-root.current] In an integration test module, relative lookup works as it does under `src`, with the test root in place of the package root. From `tests/checkout.hd`, a root file, `self.common.x` resolves to `tests.common.x`, in `tests/common/x.hd`.
14. r[module.relative.above-test-root] In an integration test module, a `super` that moves above the test root is an error. Error: `unknown-module`.

```text
# tests/checkout.hd
use self.common.{expected}   # valid: tests/common/mod.hd
use super.common.{expected}  # error: unknown-module
```

> **Why.** `self` names the current module, as Rust's `self` does. A
> file's own child modules then live in the directory of the same name,
> and a sibling is always `super.NAME`. A root file, like a crate root,
> is where its tree starts, so it has no parent.

### Single-File Programs

A **single-file program** is one source file compiled with no package,
such as a script run on its own:

```text
use std.process.ExitCode  # valid: std is the one root it may use
use self.util.{helper}    # error: unknown-module
```

1. r[module.single-file.definition] A single-file program is one source file compiled with no package. It is its own entry module, and it may use only `std`.
2. r[module.single-file.roots] In a single-file program, a use path that starts with `pkg`, `dep`, `self`, or `super` is an error, reported at the use. Error: `unknown-module`.

> **Note.** The diagnostic says that the file is in no package, rather
> than that a name is unknown.

See also: [Single Files](../cli/command-line.md#single-files) for when `hd` compiles a
file as a single-file program.

## Use Forms

A use names a single public declaration or a module namespace:

```text
use std.format.DebugWriter
use pkg.user.types
use pkg.user.types as user_types
```

A grouped use names selected public declarations:

```text
use pkg.user.types.{User, UserId}
use dep.billing.types.{UserId as BillingUserId}
```

1. r[module.use.single] A use may name a single public declaration or a module namespace.
2. r[module.use.grouped] A grouped use names selected public declarations.
3. r[module.use.trailing-comma] Grouped uses may have a trailing comma.
4. r[module.use.no-wildcard] Wildcard uses are not supported. A wildcard use is an error. Error: `syntax-error`.
5. r[module.use.no-variant] Enum variants are members, not module declarations, and cannot be used directly. Error: `direct-variant-use`.
6. r[module.use.private-or-missing] Using a private or missing declaration is a compile-time error.
7. r[module.use.missing-name] A use that names a declaration which an existing module of `std`, `pkg`, or a dependency does not declare is an error. Error: `unknown-import`.
8. r[module.use.private-name] A use that names a declaration which its module declares without `pub` is an error. Error: `private-import`.
9. r[module.use.pub-grouped] Only the grouped form accepts a `pub` prefix. A `pub` single use is an error. Error: `syntax-error`.
10. r[module.use.facade] Public facades expose selected declarations rather than module namespace aliases.
11. r[module.use.whole-module] Use declarations introduce names for the whole module and are resolved before type checking.
12. r[module.use.ambiguous] A single use that names both a module and a declaration of its parent module, as `use pkg.words`, is an error. Error: `ambiguous-import`.

```text
# in package pkg: src/words.hd beside a root declaration words
use pkg.words  # ambiguous-import: rename the module or the declaration
```

```text
use std.testing.*                 # error: syntax-error
use std.text.{nope}               # error: unknown-import
use pkg.models.{Secret}           # error: private-import
use pkg.status.{Status.Queued}    # error: direct-variant-use
pub use std.testing.assert_equal  # error: syntax-error
```

> **Note.** A qualified name that reaches a declaration through a module
> name reports these codes too, by
> [`expr.name.qualified.private`](05-expressions.md#r-expr.name.qualified.private)
> and [`expr.name.qualified.missing`](05-expressions.md#r-expr.name.qualified.missing).

## Dependency Cycles

The files of one folder may use each other in a loop, but folders must not
depend on each other in a loop:

```text
# src/shop/mod.hd
pub use pkg.shop.cart.{Cart}
pub use pkg.shop.item.{Item}

# src/shop/cart.hd: a loop with mod.hd, inside folder src/shop
use pkg.shop.{Item}
```

### Folders

A source file's **folder** is the directory that holds it, or the
directory of its child modules:

| Source file | Module | Folder |
| --- | --- | --- |
| `src/shop/mod.hd` | `shop` | `src/shop` |
| `src/shop.hd`, when `src/shop/` holds its child modules | `shop` | `src/shop` |
| `src/shop/cart.hd` | `shop.cart` | `src/shop` |
| `src/shop/orders/order.hd` | `shop.orders.order` | `src/shop/orders` |
| `src/shop.hd`, with no child modules | `shop` | `src` |
| `src/lib.hd` | the package root | `src` |

1. r[module.folder.holder] A source file's folder is the directory that holds it, except for a file with child modules.
2. r[module.folder.parent-file] A file `x.hd` whose directory `x/` beside it holds source files, its child modules, is in folder `x/`, as `x/mod.hd` would be.
3. r[module.folder.interchangeable] So `x.hd` and `x/mod.hd` are interchangeable: each is in the folder of its children.
4. r[module.folder.parent-child] A parent module and its child modules share one folder, so a parent and a child that use each other make no folder edge.
5. r[module.folder.mod-file] A `mod.hd` file is in the folder of its directory, like the other files there.
6. r[module.folder.nested] Nested directories are separate folders: a file in `src/shop/orders` is not in folder `src/shop`.

### Folder Graph

1. r[module.cycle.folder-edge] The **folder graph** of a package has an edge from folder `A` to a different folder `B` when a file in `A` uses a module in `B`. A `use` or `pub use` uses the module its path reaches, so `use pkg.shop.{Item}` and `use pkg.shop.Item` both use `shop`.
2. r[module.cycle.same-package] Only uses of the package's own modules make edges. Uses of `std` and of dependencies make none.
3. r[module.cycle.test-code] A use in [test code](#r-module.test.code) makes no edge.
4. r[module.cycle.nested] A folder and its parent or child folder are separate nodes, and an edge between them counts like any other.
5. r[module.cycle.acyclic] The folder graph must be acyclic. A folder that depends on itself through other folders is an error. Error: `folder-cycle`.
6. r[module.cycle.within-folder] Uses between files of one folder make no edge, so those files may use each other in any pattern, loops included.
7. r[module.cycle.package] The dependency graph of packages must be acyclic: a package that depends on itself, directly or through other packages, is an error. Error: `package-cycle`.

A root facade that uses a child folder, beside a shared root file that the
child uses, makes a loop of folders:

```text
# src/lib.hd, in folder src
pub use pkg.shop.{Cart}

# src/error.hd, in folder src
pub enum Error:
    Empty

# src/shop/mod.hd, in folder src/shop
use pkg.error.{Error}  # error: folder-cycle
```

> **Why.** Every signature that another file sees is written out, so files
> are checked in parallel whatever their loops. The folder rule bounds the
> largest set of files that must be compiled together at one folder. A
> folder then compiles from the signatures of the folders it uses, as a Go
> package compiles from its imports' export data.

> **Note.** Shared declarations go in a leaf folder. Moving `src/error.hd`
> to `src/error/mod.hd` keeps the module name `error`, so no `use` line
> changes, and breaks the loop above.

### Cycle Diagnostic

1. r[module.cycle.diagnostic.loop] The `folder-cycle` diagnostic must show one shortest loop of folders, with the `use` declaration that makes each edge.
2. r[module.cycle.diagnostic.size] It must show the size of the tangle: the number of folders that lie on some loop with the shown ones.
3. r[module.cycle.diagnostic.fix] It must offer a fix-it that moves a file `x.hd` on the loop to `x/mod.hd`, which keeps its module name.

See also: [Use Declarations](03-names-and-scopes.md#use-declarations),
[Initialization Order](#initialization-order).

## Prelude

The **prelude** is the implicit scope of public standard-library names that
every module has:

| Origin module | Implicit names |
| --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` |
| `std.format` | `Display`, `Debug`, `debug`, `dbg` |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` |
| `std.hash` | `Hash`, `Hasher` |
| `std.iter` | `Iterator`, `Iterable` |
| `std.console` | `Console`, `ConsoleError`, `println` |
| `std.testing` | `it` |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` |

1. r[module.prelude.in-scope] Every module implicitly has the public standard-library names in the table in scope, but for the `std.testing` row.
2. r[module.prelude.test-only] The names of the `std.testing` row are in scope only in [test code](#r-module.test.code): `tests:` blocks, test modules, and integration test modules.
3. r[module.prelude.test-only.outside] Outside test code, such a name is unknown, as an undeclared name is. Error: `unknown-name`.
4. r[module.prelude.fixed-uses] The prelude is equivalent to fixed `use` declarations.
5. r[module.prelude.no-authority] The prelude does not create ambient host authority.
6. r[module.prelude.no-shadow] A module declaration, use, type parameter, parameter, or local binding must not bind a prelude name to another declaration. Every conflict is an error. Error: `prelude-name-shadow`.
7. r[module.prelude.same-use] A `use` that names the same declaration the prelude already supplies, under the same name, is allowed and changes nothing.

```text
use std.hash.Hash                                    # ok: the prelude's Hash
use std.cmp.{Ordering as Display}                    # error: prelude-name-shadow

fn println() -> void: pass                           # error: prelude-name-shadow
fn consume(Console: i32) -> i32: Console             # error: prelude-name-shadow
fn identity[Result](value: Result) -> Result: value  # error: prelude-name-shadow

fn main() -> i32:
    let Hash: i32 = 42  # error: prelude-name-shadow
    Hash
```

```text
fn helper() -> void:
    it("outside test code"):  # error: unknown-name
        pass
```

> **Why.** Only test code registers test cases, so only test code needs
> `it`. A program built without its test code then never reaches
> `std.testing` through the prelude.

### Standard Names Outside The Prelude

1. r[module.prelude.imported] Standard traits outside this table are imported.
2. r[module.prelude.from-error] The conversion trait `std.convert.From` and the error trait `std.error.Error` are not prelude names.
3. r[module.prelude.own-from-error] A module may therefore declare its own `From` or `Error`.
4. r[module.prelude.question-from] Postfix `?` still finds the standard `From` without an import.
5. r[module.prelude.inspect] Likewise `std.inspect` declares `Inspectable`, `TypeId`, and `downcast_val`, which code imports, as in `use std.inspect.{Inspectable, TypeId}`.
6. r[module.prelude.error-inspectable] `std.error.Error` extends `Inspectable` without its users importing it.
7. r[module.prelude.function-items] `std.function` declares the function type constructors `Fn` and `SuspendFn` and the marker trait `Tuple`, which code imports where it writes them, as in `use std.function.{Fn, Tuple}`.
8. r[module.prelude.function-sugar] The function type sugar `fn(...) -> T` needs no import.
9. r[module.prelude.annotation-targets] `std.annotation` also declares `Target`, `Annotate`, and `annotate`, which code imports to limit a fact type's [target kinds](14-annotations.md#target-kinds), as in `use std.annotation.annotate`.
10. r[module.prelude.ops-literal-markers] `std.ops` declares `NumSuffix`, `num_suffix`, `StrPrefix`, `str_prefix`, and `Template`, which code imports to declare a literal function, as in `use std.ops.num_suffix` or `use std.ops.{Template, str_prefix}`.
11. r[module.prelude.no-literal-fn] The prelude supplies no literal suffix and no string prefix.
12. r[module.prelude.ops-operator-traits] `std.ops` also declares the [operator traits](05-expressions.md#operator-traits), and `Index` and `IndexSet`. Code imports one to name it, as in `use std.ops.Add`; operator syntax needs no import.
13. r[module.prelude.ops-call-traits] `std.ops` also declares `Apply` and `Update`, the traits of [callable values](05-expressions.md#callable-values). Code imports one to name it; `v()` and `v() = x` need no import.
14. r[module.prelude.num] `std.num` declares the [numeric traits](09-traits.md#numeric-traits) `Num`, `Integer`, and `Float`, which code imports, as in `use std.num.Num`.

> **Note.** More standard names outside the prelude are stdlib tier:
> the string prefix [`r`](../std/text.md#raw-text-prefix) of `std.text`,
> [`FromIterator`](../std/iter.md#collect-targets) of `std.iter`, and
> [`Duration`](../std/time.md#duration) and its suffixes of `std.time`.

See also: [Conversion Trait](09-traits.md#conversion-trait),
[Function Type Constructors](07-functions.md#function-type-constructors),
[Error Trait](09-traits.md#error-trait),
[Literal Suffixes](05-expressions.md#literal-suffixes),
[Prefixed Strings](05-expressions.md#prefixed-strings),
[Runtime Type Identity](09-traits.md#runtime-type-identity).

### Collection Type Names

1. r[module.prelude.collections] The built-in collection types are `List` and `Map`.
2. r[module.prelude.capitalized] Like every nominal type outside the primitives, they are capitalized.
3. r[module.prelude.lowercase] The lowercase names `list` and `map` are not prelude names; they resolve like any other identifier.
4. r[module.prelude.lowercase-unknown] A type written `list[i32]` is therefore an error unless a declaration in scope supplies `list`. Error: `unknown-type`.

```text
fn count() -> i32:
    let values: list[i32] = [1, 2]  # error: unknown-type
    return 2
```

### Prelude Functions

1. r[module.prelude.panic] The prelude function `panic` has the signature `panic(message: string) -> never`.
2. r[module.prelude.println] The prelude function `println` has the signature `println[T < Display](value: T) -> void $ Console`.
3. r[module.prelude.it-function] The prelude function `it` is the test-case function that [Test Cases](#test-cases) specifies.
4. r[module.prelude.debug] The prelude function `debug` has the signature `debug[T < Debug](value: T) -> string`, as [Debug Trait](09-traits.md#debug-trait) specifies.
5. r[module.prelude.dbg] The prelude name `dbg` is the function with the signature that [Debug Printing](#r-module.dbg.signature) specifies, whose body the compiler supplies.

### Debug Printing

`dbg` prints values while a bug is being chased. It is a statement that
returns nothing:

```text
# src/cart.hd
fn line_total(price: i32, qty: i32) -> i32:
    dbg(price * qty)  # prints src/cart.hd:2:5: price * qty = 1250
    price * qty + 50

fn check_order(id: i32, count: i32) -> bool:
    dbg(id, count)  # two lines, one per argument
    dbg()           # prints src/cart.hd:7:5
    id > 0 && count > 0
```

1. r[module.dbg.signature] `std.format` declares `dbg` as `@intrinsic pub fn dbg[Args < Tuple](values...: Args) -> void`, and the prelude supplies it. The declaration is ordinary hd, and the compiler supplies only its body.
2. r[module.dbg.check] A `dbg` call is checked by the plain call rules and the [vararg rules](07-functions.md#varargs): `dbg()`, `dbg(x)`, and `dbg(a, b, c)` collect their arguments into `Args` as [`fn.vararg.collect.tuple-expr`](07-functions.md#r-fn.vararg.collect.tuple-expr) says. No rule gives `dbg` a type that the declaration does not.
3. r[module.dbg.tuple-argument] `dbg((a, b, c))` is a call with one argument, a tuple, as [`fn.vararg.no-auto-spread`](07-functions.md#r-fn.vararg.no-auto-spread) says. It prints one line for the tuple, where `dbg(a, b, c)` prints one line for each argument.
4. r[module.dbg.body.print] The body evaluates the arguments from left to right and prints the value of each element of `values` on a line of its own when it is evaluated, as [Debug Values](#debug-values) says. It needs no `Debug` bound on any element type.
5. r[module.dbg.body.site] The body prints what it knows of the call site, which no ordinary hd parameter can supply, as [Debug Lines](#debug-lines) says. A direct call with only positional arguments and no explicit type arguments knows each argument's source text. Any other call knows only the location.
6. r[module.dbg.body.value] A `dbg` used as a function value, as in `let show: fn((i32, string)) -> void = dbg`, has no call site, so its body prints each element of its one tuple argument on a line of its own, with no location.
7. r[module.dbg.no-requirement] A `dbg` call needs no requirement and adds none to a row. It is valid in a pure function, in a [unit test case](#r-module.testing.unit-row.anywhere), and in generic code.
8. r[module.dbg.not-behavior] What `dbg` prints is diagnostic output, not program behavior. It is not an effect, a replay does not record it, and tools that judge a program's output ignore it.

```text
fn total(prices: List[i32]) -> i32:
    let sum = +0
    for price in prices:
        sum = sum + price
        dbg(sum)  # valid: no Console in the row
    sum

fn show_all(pair: (i32, string)) -> void:
    dbg(pair...)                           # a line per element, after the location
    dbg(values=pair)                       # the same
    let show: fn((i32, string)) -> void = dbg
    show(pair)                             # a line per element, no location
```

```text
pub fn main() -> void:
    let x: i32 = dbg(1)  # error: type-mismatch
```

> **Why.** Rust's `dbg!` writes to standard error. A print added while
> chasing a bug must not change a function's signature, so `dbg` is the
> one way to print without `Console`. Its output is for the person
> debugging, so the program's behavior never depends on it. Returning
> nothing keeps `dbg` a plain statement: deleting it never changes a
> type.

#### Debug Lines

1. r[module.dbg.line] Each argument of a direct `dbg` call with positional arguments prints one line: the call's location, `: `, the argument's source text, ` = `, and its value.
2. r[module.dbg.line.bare] A call that has no per-argument source text, as [`module.dbg.body.site`](#r-module.dbg.body.site) says, prints one line for each element of `values`: the call's location, `: `, and the element's value.
3. r[module.dbg.location] The location is `FILE:LINE:COLUMN`: the file as diagnostics name it, and the line and column where the call begins.
4. r[module.dbg.stream] A `dbg` line goes to the program's debug output, which a program that `hd` runs writes to standard error. [Debug Output](../cli/command-line.md#debug-output) says where it goes in tests and the REPL.

#### Debug Values

1. r[module.dbg.value.debug] A value whose static type implements `Debug` prints as its `Debug` text, as [`debug`](#r-module.prelude.debug) returns it.
2. r[module.dbg.value.structural] A value of any other static type prints its structure, as the table below says. Each part prints by these same rules, so a part whose type implements `Debug` prints through it.
3. r[module.dbg.value.no-bound] Neither rule needs a trait bound, so every value prints.
4. r[module.dbg.value.source] A printed value is hd source wherever its parts have a source form, so it pastes back as an expression that builds an equal value, as [`std-format.debug.source`](../std/format.md#r-std-format.debug.source) specifies. A string prints as a string literal, and a char as a char literal.
5. r[module.dbg.value.generic] Inside generic code, a value whose static type mentions a type parameter prints through `Debug` when that type implements `Debug` under the bounds in scope, as for `T < Debug`. Otherwise what it prints is implementation-defined.

| Rule | Static type | Prints |
| --- | --- | --- |
| r[module.dbg.value.data] Data type | a `data` type | its name and each field with its name, private fields included, as `Point { x: 1, y: 2 }` |
| r[module.dbg.value.variant] Enum | an `enum` | its variant as hd source constructs it, qualified by the enum, with positional payloads first and named ones as `name=value`: `Shape.Dot`, `Shape.Pair(1, 2)`, or `Shape.Circle(radius=2.0)` |
| r[module.dbg.value.newtype] Newtype | a [newtype](04-type-system.md#newtypes) | its name and its base value, as `Meters(1.5)` |
| r[module.dbg.value.composite] Built-in composite | a list, map, tuple, `T?`, or `Result` | as its standard `Debug` implementation writes it, as `[1, 2]`, `{"a": 1}`, `(1, "a")`, `Option.Some(1)`, or `Result.Ok(1)` |
| r[module.dbg.value.function] Function | a function or closure | `<fn name(i32) -> i32>` when the argument names a function, else `<fn(i32) -> i32>` |
| r[module.dbg.value.handle] Handle | a live runtime handle, such as a suspension | `<handle>` |

```text
data Account:
    owner: string
    balance: i32  # private, and dbg shows it

fn audit(account: Account) -> Account:
    dbg(account)  # prints ...: account = Account { owner: "Ada", balance: 120 }
```

> **Why.** A type should not need `Debug` before it can be inspected, and
> the person debugging needs every field. A type's own `Debug` still wins,
> so a type that hides a secret can print it masked.

#### Large Values

1. r[module.dbg.layout] A value prints on its line when the whole line fits in about 80 columns. Otherwise it prints over several lines, each field or entry on its own line and indented by its depth.
2. r[module.dbg.cycle] A value that is reached again while it is being printed, through a cycle of references, prints `<cycle>`.
3. r[module.dbg.limit.entries] A list or map prints at most its first 100 entries, then `… N more` for the N entries it leaves out.
4. r[module.dbg.limit.depth] A part nested more than 10 levels deep prints `…`.
5. r[module.dbg.limit.string] A string longer than 1000 characters prints its first 1000 characters, then `…`.

> **Note.** The limits keep one `dbg` call of a large or cyclic value
> readable, and keep it from running without end.

#### Release Builds And Dependencies

```sh
hd build --release   # error: dbg-in-release; the fix-it deletes the dbg statement
```

1. r[module.dbg.release] A reference to `dbg` in the user's own code, a call or a function value, is an error in a release build, one of the [build profiles](04-type-system.md#integer-arithmetic). Error: `dbg-in-release`.
2. r[module.dbg.release.delete] The fix-it of a call deletes the whole `dbg(...)` statement. A reference that is not a call has no fix-it, because the code that uses the value must change.
3. r[module.dbg.release.debug-build] A debug or test build accepts a reference to `dbg` with no warning.
4. r[module.dbg.own-code] The user's own code is the root package, every member of its [workspace](#workspaces), and every package that a [path requirement](#path-requirements) reaches.
5. r[module.dbg.dependency] In a package fetched for a version requirement, a `dbg` call prints nothing in every build. It still evaluates its arguments, as [`module.dbg.body.print`](#r-module.dbg.body.print) says.
6. r[module.dbg.dependency.warning] A build that compiles such a call warns once per fetched package, naming the package. Warning: `dbg-in-dependency`.

> **Why.** A `dbg` call is never meant to ship, so a release build stops
> it and its fix-it removes it. Removing a statement changes no types. A published package that forgot one must
> not print into its users' programs or break their release builds.

#### Debug Text Hint

1. r[module.dbg.debug-hint] An expression statement that calls the prelude function `debug` discards the text that `debug` returns. It warns, and the message suggests `dbg`. Warning: `unused-debug-text`.

```text
fn trace(prices: List[i32]) -> void:
    debug(prices)  # warning: unused-debug-text
```

See also: [Debug Trait](09-traits.md#debug-trait),
[Debug Output](../cli/command-line.md#debug-output),
[Build Profiles](../cli/command-line.md#build-profiles).

### Console

The standard console surface includes:

```text
trait Console:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]

    fn write_error_line!(mut self, text: string) -> Result[void, ConsoleError]:
        self.write_line!(text)

pub enum ConsoleError:
    Closed
```

1. r[module.console.host-trait] `Console` is a host capability trait.
2. r[module.console.write-line-mut] `write_line!` takes `mut self`, so `Console` is a mutable requirement trait and a provider may record what it writes.
3. r[module.console.write-error-line] `write_error_line!` writes one line of error output, which a host keeps apart from the output of `write_line!`.
4. r[module.console.write-error-line.default] Its default body calls `write_line!` with the same text, so a provider that does not override it records both kinds of line together.
5. r[module.console.error] `ConsoleError` is its standard boundary-safe error type, and `ConsoleError` implements `Display`.
6. r[module.console.error.closed] `ConsoleError` is a public enum with one variant, the payload-free `Closed`, so code may build `ConsoleError.Closed`.
7. r[module.console.println] Thus `println` is convenient to name but not a global host API. Each call must be covered by a `Console` requirement row or a lexical provider scope. Error: `missing-requirement`.
8. r[module.console.println-write] A call `println(value)` calls `write_line!(value.to_string())` on the `Console` provider that covers the call.
9. r[module.console.println-drive.block-on] `println` drives that `write_line!` call with [`block_on`](11-requirements-and-suspension.md#r-req.drive.block-on), exactly as `block_on` drives a stored suspension, and returns after the call completes.
10. r[module.console.println-drive.pending] When a poll of that call returns `Pending` on a host write, `println` keeps driving the call until it finishes, as `block_on` does.
11. r[module.console.println-non-suspending] `println` stays non-suspending: a call of it is not a bang call and needs no driver context.
12. r[module.console.println-error] If that `write_line!` call returns `.Err(ConsoleError)`, `println` panics.
13. r[module.console.println-std] `println` is an ordinary function of the standard library's prelude.
14. r[module.console.println-panics] Its panics are ordinary panics that `std` raises, each with a message `std` defines. No panic category is specific to `println`.
15. r[module.console.println-error.category] The `.Err` panic is an ordinary `panic` call in `std`, so its category is `explicit-panic`. Panic: `explicit-panic`.
16. r[module.console.println-block-on] `println` follows every rule of [`block_on`](11-requirements-and-suspension.md#r-req.drive.block-on): its forbidden contexts, their transitive ban, and its behavior under an active driver.
17. r[module.console.println-block-on.under-driver] So a `println` call while a driver is active, as in `main!` or a test body, writes its line and returns.
18. r[module.console.println-block-on.contexts] A `println` call in a `defer` suite, a default expression, or a fact expression, directly or transitively, is an error. Error: `suspension-forbidden-context`.
19. r[module.console.println-script] A `println` call at the top level of a [script](#r-module.init.script) is valid, since only non-entry module initialization bans `block_on`.

```text
pub fn main() -> void:
    println("missing")  # error: missing-requirement

fn report() -> void $ Console:
    defer:
        println("done")  # error: suspension-forbidden-context
    pass
```

A recording provider receives each line that `println` writes:

```text
data BufferConsole:
    lines: mut List[string]

impl Console for BufferConsole:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:
        self.lines.push(text)
        .Ok(())

fn greet(name: string) -> void $ Console:
    println("hello, ${name}")

pub fn main() -> void $ Console:
    let mut console = BufferConsole { lines: [] }
    $.with(Console=console):
        greet("Ada")
    println("recorded: ${console.lines[0]}")  # recorded: hello, Ada
```

Suspending code, such as `main!` or a test body, may also call
`write_line!` directly:

```text
pub fn main!() -> Result[void, ConsoleError] $ Console:
    let mut console = $.use(Console)
    console.write_line!("done")
```

> **Why.** Rust's `println!` and Go's `fmt.Println` are synchronous, so
> every caller keeps its signature. Rust's `println!` also panics when the
> write to standard output fails. `println` is ordinary `std` code that
> drives its write as `block_on` does, so it has `block_on`'s rules and
> no language rule of its own.

> **Note.** Code that must handle a console error calls `write_line!`
> directly.

> **Note.** `eprintln`, which writes through `write_error_line!`, and the
> input trait `ConsoleInput` are stdlib tier:
> [Console](../std/console.md) in `std.console`.

See also: [Driving A Stored Suspension](11-requirements-and-suspension.md#driving-a-stored-suspension),
[Mutable Providers](11-requirements-and-suspension.md#mutable-providers).

### Processes

`std.process` declares the host capability trait that starts a program:

```text
pub data ProcessOutput:
    pub stdout: string
    pub stderr: string
    pub status: i32

pub enum ProcessError:
    NotFound
    PermissionDenied
    Other(message: string)

pub trait Process:
    fn run!(mut self, program: string, args: List[string], stdin: string) -> Result[ProcessOutput, ProcessError]
```

1. r[module.process.declares] `Process` is a host capability trait that `std.process` declares, with the data type `ProcessOutput` and the enum `ProcessError`.
2. r[module.process.run] `run!(program, args, stdin)` starts the program named `program` with the arguments `args` and the standard input `stdin`, and completes when that program exits.
3. r[module.process.ok-output] When the program starts, `run!` returns `.Ok` of a `ProcessOutput`. It holds the text the program wrote to standard output and to standard error, and the status it exited with.
4. r[module.process.nonzero-ok] A non-zero exit status is still an ordinary `.Ok` result; the caller reads it from `status`.
5. r[module.process.not-found] `run!` returns `.Err(ProcessError.NotFound)` when its provider has no program named `program`.
6. r[module.process.permission-denied] It returns `.Err(ProcessError.PermissionDenied)` when the program exists but the host does not allow starting it.
7. r[module.process.other] It returns `.Err(ProcessError.Other(message))` when the program cannot start for any other reason, with a message that says why.
8. r[module.process.error-traits] `ProcessError` implements `Eq`, `Debug`, `Display`, and [`Error`](09-traits.md#error-trait).
9. r[module.process.mut] `run!` takes `mut self`, so `Process` is a mutable requirement trait, as `Console` is, and a provider may record what it runs.
10. r[module.process.import.names] `Process`, `ProcessOutput`, and `ProcessError` are not prelude names; code imports them from `std.process`.

A scripted provider answers each program from a table:

```text
use std.process.{Process, ProcessError, ProcessOutput}

data ScriptedProcess:
    outputs: Map[string, ProcessOutput]

impl Process for ScriptedProcess:
    fn run!(mut self, program: string, args: List[string], stdin: string) -> Result[ProcessOutput, ProcessError]:
        match self.outputs.get(program):
            .Some(output) => .Ok(output)
            .None => .Err(.NotFound)

fn version!() -> string $ Process:
    match $.use(Process).run!("git", ["--version"], ""):
        .Ok(output) => output.stdout
        .Err(.NotFound) => "no git"
        .Err(error) => "git did not start: ${error}"
```

> **Note.** Which programs a provider starts is the provider's choice. The
> test runner binds one whose programs are the package's executables
> ([`cli.test.process`](../cli/command-line.md#r-cli.test.process)).

> **Note.** The `Display` text of each `ProcessError` variant and the
> stdlib provider `ScriptedProcess` are stdlib tier:
> [Process](../std/process.md) in `std.process`.

> **Why.** Starting a program is a host capability like any other, so a
> function that starts one states it in its row, and a test can install a
> scripted provider. A missing program and a refused start are failures,
> not absence, so `run!` returns a `Result`, as the file system traits do.
> A program that ran and exited with an error still ran, so its status is
> ordinary output.

### Value-Category Traits

1. r[module.prelude.any-subtraits] `AnyVal` and `AnyRef` are the two sealed marker subtraits of `Any`.
2. r[module.prelude.anyref] `AnyRef` is implemented by data values, stored enum values (optionals included), lists, maps, dynamic trait values, and `Any`. It is also implemented by closures, suspensions, payload-free enum values with canonical variant identity, and those runtime handles that have identity.
3. r[module.prelude.anyref-not] `AnyRef` is not implemented by primitives or tuples.
4. r[module.prelude.anyval-types] `AnyVal` is implemented by exactly the primitives, tuples (`void` included), and newtypes whose base type implements `AnyVal`.
5. r[module.prelude.newtype-category] A newtype implements `AnyRef` exactly when its base type does.
6. r[module.prelude.any-sealed] User code cannot implement either.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any),
[`types.sealed.never`](04-type-system.md#r-types.sealed.never) for `never`,
[Sealed Traits](09-traits.md#sealed-traits).

### Built-In Methods

The following built-in methods are normative:

| Receiver | Methods |
| --- | --- |
| `string` | `len(self) -> usize`; `chars(self) -> mut Iterator[char]`; `char_indices(self) -> mut Iterator[(usize, char)]`; `bytes(self) -> mut Iterator[u8]`; `slice(self, start: usize, end: usize) -> string` |
| `List[T]` | `len(self) -> usize`; `iter(self) -> mut Iterator[T]` |
| `mut List[T]` | `push(mut self, value: T) -> void`; `pop(mut self) -> T?`; `insert(mut self, index: usize, value: T) -> void`; `remove_at(mut self, index: usize) -> T`; `clear(mut self) -> void` plus the readonly methods |
| `Map[K, V]` | `len(self) -> usize`; `get(self, key: K) -> V?` |
| `mut Map[K, V]` | `remove(mut self, key: K) -> V?` plus the readonly methods |
| `Display` | `to_string(self) -> string` |

1. r[module.method.normative] The built-in methods in the table are normative.
2. r[module.method.usize-sizes] Lengths, indices, and byte offsets use [`usize`](04-type-system.md#the-usize-type).
3. r[module.method.no-set] No `set` type is part of the core prelude.
4. r[module.method.list-pop] `pop` removes the last element of the list and returns it as `.Some`.
5. r[module.method.list-pop.empty] `pop` on an empty list returns `.None` and leaves the list unchanged.
6. r[module.method.list-insert] `insert(index, value)` puts `value` at `index` and moves each element from `index` on one place up.
7. r[module.method.list-insert.end] `insert` at `len()` appends `value`, as `push` does.
8. r[module.method.list-insert.range] An `insert` index greater than `len()` is a checked runtime panic. Panic: `index-out-of-bounds`.
9. r[module.method.list-remove-at] `remove_at(index)` removes the element at `index`, moves each later element one place down, and returns the removed element.
10. r[module.method.list-remove-at.range] A `remove_at` index that is not less than `len()` is a checked runtime panic. Panic: `index-out-of-bounds`.
11. r[module.method.list-clear] `clear` removes every element, so `len()` is 0 after it.

| Call on `[1, 2, 3]` | List after |
| --- | --- |
| `insert(1, 9)` | `[1, 9, 2, 3]` |
| `insert(3, 9)` | `[1, 2, 3, 9]` |
| `remove_at(0)`, which returns `1` | `[2, 3]` |
| `clear()` | `[]` |

> **Note.** `push`, `insert`, `remove_at`, and a `pop` or `clear` that
> removes an element change the list's length, so each invalidates the
> list's iterators by
> [`flow.for.invalidate`](06-control-flow.md#r-flow.for.invalidate).

See also: [Text](../std/text.md#string-methods) for the string methods above
these, such as `trim` and `split`, [Iterators](../std/iter.md#list-and-optional-map)
for `map` on a list or an optional, and [Collections](../std/collections.md)
for the list method `view`.

#### Map Complexity

1. r[module.map.complexity] Map lookup, insertion, and removal take expected amortized O(1) time.
2. r[module.map.complexity.operations] This covers `get`, `remove`, reading `entries[key]`, and inserting or replacing through `entries[key] = value`.
3. r[module.map.complexity.step] Each call of the key type's `Hash` or `Eq` implementation counts as one step.

See also: [Indexing](05-expressions.md#indexing).

#### String Methods

A string method takes and returns byte offsets, and iteration over a string
is explicit:

```text
fn first_word(text: string) -> string:
    for (offset, letter) in text.char_indices():
        if letter == ' ':
            return text.slice(0, offset)
    text
```

1. r[module.string.utf8] String methods operate on valid UTF-8 strings without locale.
2. r[module.string.byte-offsets] Every position that a string method takes or returns is a byte offset.
3. r[module.string.chars] `chars` yields the string's scalar values in order, each as a `char`.
4. r[module.string.char-indices] `char_indices` yields each scalar value in order, as `(offset, char)`, where `offset` is the byte offset at which its encoding starts.
5. r[module.string.bytes] `bytes` yields the string's bytes in order, each as a `u8`.
6. r[module.string.slice] `slice(start, end)` returns the bytes from offset `start` up to but not including offset `end`, in constant time.
7. r[module.string.slice.shared] The result shares the original string's bytes rather than copying them.
8. r[module.string.slice.bad-offset] An offset that is not a [scalar boundary](04-type-system.md#r-types.string.boundary), inside a scalar's encoding or past the end, is a checked runtime panic. Panic: `index-out-of-bounds`.
9. r[module.string.slice.reversed] A `start` greater than `end` is a checked runtime panic, even when both are scalar boundaries. Panic: `index-out-of-bounds`.

> **Note.** `char_indices` gives the offsets that Go's `range` over a
> string gives. `text[start..end]` gives the same substring as
> `text.slice(start, end)`, through [Slicing](05-expressions.md#slicing).

## Standard Testing

`std.testing` exports these normative assertion functions:

```text
fn assert(condition: bool, reason: string) -> void
fn assert_equal[T < Eq & Debug](actual: T, expected: T, reason: string) -> void
```

1. r[module.testing.exports] `std.testing` exports the normative assertion functions `assert` and `assert_equal` with the signatures above.
2. r[module.testing.reason] `reason` is required and must explain the checked condition.
3. r[module.testing.assert-panic] A failed assertion causes a runtime panic, inside a test case or not. Panic: `assertion-failed`.
4. r[module.testing.shows-reason] A failed `assert` or `assert_equal` shows its `reason`.
5. r[module.testing.uses-eq] `assert_equal` uses `Eq.eq`.
6. r[module.testing.no-implicit-eq] `assert_equal` does not grant implicit equality to its argument type. An argument type without `Eq` is an error. Error: `missing-eq`.
7. r[module.testing.assert-equal-debug] `assert_equal` also requires `T < Debug`, and a failure shows both values as `debug` renders them. A type without `Debug` is an error. Error: `unsatisfied-trait-bound`.

```text
use std.testing.assert_equal

@derive(Debug)
data Error:
    message: string

tests:
    it("result equality needs Eq"):
        let actual: Result[i32, Error] = .Ok(1)
        assert_equal(actual, .Ok(1), reason="values match")  # error: missing-eq
```

### Test Cases

A call of the prelude function `it` registers one test case:

```text
use std.testing.assert_equal

fn add(a: i32, b: i32) -> i32: a + b

tests:
    it("adds two values"):
        assert_equal(add(2, 3), 5, reason="small sums")

    it("adds large values", ignore="slow on shared runners"):
        assert_equal(add(1000, 2000), 3000, reason="large sums")
```

`std.testing` declares `it` as an ordinary function:

```text
pub fn it[T < Termination, $R](name: string, ignore: string? = .None, expect_panic: string? = .None,
                             timeout: Duration? = .None, body: fn!() -> T $ R) -> void $ R
```

1. r[module.testing.it-function] `it` is an ordinary function with the signature above, which `std.testing` declares and the prelude supplies. Each call in test position registers one **test case**.
2. r[module.testing.it.form] A call passes the test name as its one positional argument, then optional named options, then the body as its final argument, usually as a trailing block.
3. r[module.testing.it.body] The body has type `fn!() -> T $ R` with `T < std.process.Termination`, so a trailing block body is a suspending closure.
4. r[module.testing.it.name] The name must be a string literal without interpolation. Any other name is an error. Error: `non-literal-test-argument`.
5. r[module.testing.it.options-strings] The named options are those of the signature above: `ignore`, `expect_panic`, and `timeout`. An `ignore` or `expect_panic` value must be a string literal without interpolation. Any other value for them is an error. Error: `non-literal-test-argument`.
6. r[module.testing.it.unknown-option] Any other named argument is an error. Error: `unknown-named-argument`.
7. r[module.testing.test-position] **Test position** is the top level of a `tests:` block, of a [test module](#test-modules), or of an integration test module.
8. r[module.testing.position-statements] Every statement in test position must be a call of a **test registration function**: `it`, `it_each`, `it_prop`, or `it_prop_with` ([Registration Functions](#registration-functions)). Any other statement is an error. Error: `invalid-test-statement`.
9. r[module.testing.direct-call] A test registration function may be used only as such a direct call in test position. Any other use, including a call elsewhere or a use as a value, is an error. Error: `misplaced-test-case`.
10. r[module.testing.it.unique] Two test cases of one module must not have the same name. Error: `duplicate-test-name`.

| Rule | Option | Value | Effect |
| --- | --- | --- | --- |
| r[module.testing.option.ignore] Ignore | `ignore` | a reason | The runner does not run the test case and reports it as ignored, with the reason. |
| r[module.testing.option.expect-panic] Expected panic | `expect_panic` | a [panic category](06-control-flow.md#panic-categories) | The test case passes only when its body panics with that category. |

1. r[module.testing.option.expect-panic.known] An `expect_panic` value that names no [panic category](06-control-flow.md#r-flow.panic.stable-categories) is an error. Error: `unknown-panic-category`.

```text
fn name_of() -> string: "computed"

tests:
    let shared: i32 = 0  # error: invalid-test-statement

    it("counts"):
        pass

    it("counts"):  # error: duplicate-test-name
        pass

    it(name_of()):  # error: non-literal-test-argument
        pass

    it("rejects a category", expect_panic="index-out-of-range"):  # error: unknown-panic-category
        pass

    it("waits", timeout="5s"):  # error: type-mismatch
        pass

    fn helper() -> void:
        it("nested"):  # error: misplaced-test-case
            pass

    fn register() -> void:
        make := it  # error: misplaced-test-case
```

> **Note.** `it` is a prelude name, so a module cannot declare, use, or bind
> another `it` ([Prelude](#prelude)). A call spelled `it(...)` always
> registers a test case, and a tool can list test cases without running
> them.

> **Why.** `it` is an ordinary function because its options may precede
> its final body parameter ([Default Values](07-functions.md#default-values)).
> Allowing only direct calls in test position keeps every test case
> statically listable.

> **Note.** The `timeout` parameter of `it` has type `std.time.Duration?`.
> `Duration` is a stdlib-tier type ([Time](../std/time.md#duration)): the
> language tier names it in this signature only and specifies none of its
> values.

See also: [Test Timeout](../std/testing.md#test-timeout) in the stdlib tier,
for what the `timeout` option does.

### Registration Functions

`std.testing` declares three more test registration functions. `it_each`
registers one test case per row:

```text
pub fn it_each[A, T < Termination, $R](name: string, rows: List[A], ignore: string? = .None,
                                      expect_panic: string? = .None, timeout: Duration? = .None,
                                      body: fn!(A) -> T $ R) -> void $ R
```

`it_prop` and `it_prop_with` register one property test case each:

```text
pub fn it_prop[T < Arbitrary & Debug, R < Termination](name: string, ignore: string? = .None,
                                                       expect_panic: string? = .None, timeout: Duration? = .None,
                                                       cases: usize = 100, shrink: usize = 500, examples: List[T] = [],
                                                       prop: fn!(T) -> R) -> void
pub fn it_prop_with[T < Debug, R < Termination](name: string, gen: fn(mut Choices) -> T, ignore: string? = .None,
                                                expect_panic: string? = .None, timeout: Duration? = .None,
                                                cases: usize = 100, shrink: usize = 500, examples: List[T] = [],
                                                prop: fn!(T) -> R) -> void
```

1. r[module.testing.reg.functions] `it_each`, `it_prop`, and `it_prop_with` are test registration functions with the signatures above, which `std.testing` declares. The test-position rules of `it` apply to them.
2. r[module.testing.reg.import] None of the three is a prelude name. Code imports them from `std.testing`, as in `use std.testing.it_each`.
3. r[module.testing.reg.identity] A call is a call of a test registration function by declaration identity, not by spelling. The test-position rules and the rules of this section apply to it under any spelling.
4. r[module.testing.reg.identity.spellings] So a bare `it_each(...)`, a call of a renamed import such as `use std.testing.{it_each as each}`, and `testing.it_each(...)` after `use std.testing` all call `it_each`.
5. r[module.testing.reg.identity.all] The same holds for `it`, `it_prop`, and `it_prop_with`. `it` is a prelude name, and code may also import it under another name or call it as `testing.it`.
6. r[module.testing.reg.row-body-closure] The body of `it_each` has a parameter, so it is an explicit `fn!` closure, not a trailing block. After omitted options, a call passes it by name, as in `body=fn!(value: i32): ...`.
7. r[module.testing.reg.row-name-clash] Another test case of the module must not be named `name[i]` for any index `i` of an `it_each` call's rows. Error: `duplicate-test-name`.
8. r[module.testing.reg.prop-registers] A call of `it_prop` or `it_prop_with` in test position registers one property test case.
9. r[module.testing.reg.name] The name of an `it_each`, `it_prop`, or `it_prop_with` call must be a string literal without interpolation. Any other name is an error. Error: `non-literal-test-argument`.
10. r[module.testing.reg.options] Each takes the options of `it`, `ignore`, `expect_panic`, and `timeout`, under the same rules.
11. r[module.testing.reg.body-result] The body's result follows [Propagation In Test Blocks](05-expressions.md#propagation-in-test-blocks): `void`, or `Result[void, Error]` when it uses `?`.
12. r[module.testing.reg.body-closure-result] The body closure of an `it_each`, `it_prop`, or `it_prop_with` call that writes no result type gets its result type by [`expr.try.test.with-try`](05-expressions.md#r-expr.try.test.with-try) and [`expr.try.test.without-try`](05-expressions.md#r-expr.try.test.without-try), as a trailing block given to `it` does.
13. r[module.testing.reg.prop-debug] `it_prop` and `it_prop_with` require `T < Debug`. A property whose input type does not implement `Debug` is an error. Error: `unsatisfied-trait-bound`.

```text
use std.testing.it_each

fn label() -> string: "halves"

tests:
    it_each("doubles", [1, 2], body=fn!(value: i32): pass)

    it("doubles[0]"):  # error: duplicate-test-name
        pass

    it_each(label(), [1, 2], body=fn!(value: i32): pass)  # error: non-literal-test-argument
```

Every spelling of a registration function registers test cases:

```text
use std.testing
use std.testing.{it as check, it_each as each}

tests:
    check("adds"):
        pass

    each("doubles", [1, 2], body=fn!(value: i32): pass)

    testing.it_each("halves", [2, 4], body=fn!(value: i32): pass)

fn helper() -> void:
    each("rows", [1, 2], body=fn!(value: i32): pass)  # error: misplaced-test-case
```

> **Note.** `Arbitrary`, `Choices`, and `Duration` are stdlib-tier types
> ([Property Tests](../std/testing.md#property-tests),
> [Time](../std/time.md#duration)). The language tier names them in these
> signatures only and specifies none of their members.

> **Why.** The compiler lists every test case without running it, so it
> knows each registration function by its declaration, where its calls stand, and
> which of their arguments must be literals.

See also: [Table-Test Rows](../std/testing.md#table-test-rows), for how an
`it_each` call expands and names its rows, and
[Property Tests](../std/testing.md#property-tests), for how the runner
generates, shrinks, and reports a property's inputs.

### Test Outcomes

Each test case runs alone and passes or fails by its result:

```text
fn first(items: List[i32]) -> i32: items[0]

tests:
    it("an empty list has no first item", expect_panic="index-out-of-bounds"):
        _ := first([])
```

1. r[module.testing.instance] Each test case runs in its own fresh program instance, after module initialization.
2. r[module.testing.no-reuse] Instances are not reused between test cases.
3. r[module.testing.driven] The runner drives the body's suspension to completion, as the host drives `main!`.
4. r[module.testing.runner-provider] A test run's profile also binds the host capability trait `std.testing.TestRunner`, and the runner binds a provider of it for the body of every test case.
5. r[module.testing.unit-row.test-runner] A test case in a `tests:` block or a test module gets no other host provider. Its body's requirement row may hold `TestRunner` and no other key, so every other requirement comes from a `$.with` provider scope. Error: `missing-requirement`.
6. r[module.testing.unit-row.anywhere] A **unit test case** is a test case in a `tests:` block or a test module. Rule `module.testing.unit-row.test-runner` holds for it wherever its file lies.
7. r[module.testing.unit-row.places] So a `tests:` block under the source root, in a [task](../cli/command-line.md#tasks) or a shared task module, or in a [single-file program](#single-file-programs) gets `TestRunner` alone.
8. r[module.testing.kind-decides] The kind of a test case decides what the runner binds for it, never the directory of its file. Only an integration test case and a [doc test](#doc-tests) get the profile's providers.
9. r[module.testing.profile] A test run compiles against one [runtime profile](#runtime-profiles), the default profile unless the run selects another.
10. r[module.testing.integration-row] For a test case in an integration test module, the runner binds the body's requirement row from that profile, as the host binds the row of `main`.
11. r[module.testing.integration-row.unbound] An integration test case whose body needs a trait that the profile does not bind must bind it explicitly with `$.with`.
12. r[module.testing.integration-row.unbound.error] Otherwise checking reports `missing-requirement` with the hint `bind it with $.with(Trait=...)`.
13. r[module.testing.pass] A test case passes when its body completes and `report()` on its result returns `ExitCode(0)`.
14. r[module.testing.fail] It fails when `report()` returns another code or when its body panics, including by a failed assertion.
15. r[module.testing.err-print] When the result holds an `.Err`, the runner prints the error as [Entry Results](#entry-results) describes.
16. r[module.testing.expect-panic-fail] With `expect_panic`, the test case instead fails when its body completes or panics with another category.

```text
trait Clock:
    fn now(self) -> i32

data FixedClock:
    time: i32

impl Clock for FixedClock:
    fn now(self) -> i32: self.time

fn stamp() -> i32 $ Clock:
    $.use(Clock).now()

tests:
    it("stamps with a fake clock"):
        $.with(Clock=FixedClock { time: 7 }):
            _ := stamp()

    it("has no host clock"):
        _ := stamp()  # error: missing-requirement
```

> **Why.** Source cannot catch a panic, so only the runner can judge an
> expected one, from the panic's stable category. Unit tests run on fakes
> alone, so they pass on every machine; only integration tests reach real
> providers.

> **Why.** The runner is not a resource that a fake would replace, so a
> unit test that reaches it through `TestRunner` still passes on every
> machine.

> **Why.** What a test may touch follows from what it tests, not from
> where its code lives. A task's helper is checked on fakes as a library
> function is, and a test that needs the real system is an integration
> test.

> **Note.** `TestRunner` is a stdlib-tier trait
> ([Runner Capabilities](../std/testing.md#runner-capabilities)). The
> language tier names it in these rules only and specifies none of its
> methods.

> **Note.** The `missing-requirement` error in a unit test case suggests
> a std fake for the missing trait, such as `ManualClock` for `Clock`
> ([Unit Test Providers](../std/testing.md#unit-test-providers)), or moving
> the test case to the test root.

See also: [Propagation In Test Blocks](05-expressions.md#propagation-in-test-blocks),
[Exit Status](#exit-status).

### Snapshots

`std.testing` declares `snapshot`, which compares text with an expectation
written in the source:

```text
pub fn snapshot(text: string, expect: string = "") -> void
```

1. r[module.testing.snapshot.literal] An `expect` argument must be a string literal without interpolation. Any other value is an error. Error: `non-literal-test-argument`.

```text
use std.testing.snapshot

fn greeting(name: string) -> string: "hello, " + name

tests:
    it("greets by name"):
        snapshot(greeting("Ada"), expect="hello, Ada")

    it("computes the expectation"):
        snapshot(greeting("Ada"), expect=greeting("Ada"))  # error: non-literal-test-argument
```

> **Why.** A literal `expect` lets a tool rewrite it in place, so an update
> run records a new or changed expectation, and an empty `expect` is filled
> on the first update.

See also: [Snapshot Files](../std/testing.md#snapshot-files) in the stdlib
tier, for how `snapshot` and `snapshot_file` compare text, update runs, and
where snapshot files live.

## Module Initialization

This section defines which modules initialize, in what order, and what their
top-level code may do.

1. r[module.init.script] A **script** is an entry module whose top-level executable statements are the entry behavior and which has no `main` declaration.
2. r[module.init.entry-module.selected] An **entry module** is the module that a program starts from: a module that the toolchain selects in a package, or the file of a single-file program.
3. r[module.init.statements-and-main] If an entry module contains both top-level statements and `main`, its top-level statements initialize the module first and then the runtime invokes `main`. That form is an executable entry module, not a script.
4. r[module.init.program-instance] A **program instance** is one instantiated Wasm module graph together with its module storage, provider bindings, and execution state.

### Initialization Order

1. r[module.init.use-graph] Before execution, the compiler resolves the use graph reachable from the selected script or executable entry module. The graph may have loops inside one folder.
2. r[module.init.group] An **initialization group** is a strongly connected component of that graph: one module, or the modules that use each other in a loop.
3. r[module.init.group.once] Every reachable group is initialized exactly once per program instance, after every other group it uses has been initialized.
4. r[module.init.group.ready-order] When several groups are otherwise ready, the least fully qualified module identity in each group orders them lexicographically.

> **Why.** Ordering by module identity makes initialization independent of
> filesystem enumeration.

#### Order Inside A Group

Top-level statements of a group run in dependency order, then in file
order, as Go orders the variables of one package:

```text
# src/shop/catalog.hd
use super.prices

let featured = prices.price_of("tea")  # runs after prices.markup

# src/shop/prices.hd
use super.catalog

let markup = 5

pub fn price_of(sku: string) -> i32:
    catalog.base_price(sku) + markup
```

1. r[module.init.group.dependency] A top-level statement depends on each top-level binding of its group in its transitive read set. The read set is computed as for [definite initialization](#definite-initialization), across every module of the group.
2. r[module.init.group.step] A group initializes one top-level executable statement at a time. Each step runs the earliest remaining statement whose dependencies are all initialized.
3. r[module.init.group.earliest] Statements are ordered by fully qualified module identity, then by source position.
4. r[module.init.group.cycle] When statements remain and none of them is ready, they form an initialization cycle, which is an error. Error: `top-level-read-before-initialization`.

> **Note.** In a group of one module, definite initialization makes every
> statement ready in turn, so its statements run in source order.

### Top-Level Statements

1. r[module.init.declarations] Within one module, named declarations are available before initialization.
2. r[module.init.source-order-single] In a group of one module, top-level executable statements run in source order. A larger group follows [Order Inside A Group](#order-inside-a-group).
3. r[module.init.uses] Use declarations do not execute as statements.
4. r[module.init.binding] Top-level bindings are initialized at their statement, before later function bodies may access them.
5. r[module.init.storage] Their storage remains available to functions in that module for the lifetime of the program instance.
6. r[module.init.no-step] A module with no top-level executable statements has no observable initialization step.

### Definite Initialization

1. r[module.init.definite] Before accepting a top-level executable statement, the compiler checks the transitive read set of each function or closure it references. Every top-level binding in that set must already be initialized. Error: `top-level-read-before-initialization`.
2. r[module.init.definite.implicit] References passed as values and functions reached by trait dispatch, interpolation, iteration, or another implicit call are included.
3. r[module.init.definite.dispatch] A trait method call through a generic bound or a dynamic trait value reaches every implementation of that method in the module.
4. r[module.init.definite.whole-module] This definite-initialization check covers the whole module value-flow and call graph.
5. r[module.init.definite.local] The check is local to one module.

```text
first := apply(first_name)  # error: top-level-read-before-initialization
let names: List[string] = ["Ada"]

fn apply(callback: fn() -> string) -> string:
    callback()

fn first_name() -> string:
    names[0]
```

> **Note.** An implementation can compute the check from a per-function
> summary of the top-level bindings each function reads, combined bottom-up
> over the module's call graph. It never needs another module's function
> bodies. Ordering a larger group combines the same summaries across the
> group's modules, after their bodies are checked.

### Entry Behavior

1. r[module.init.script-body] After dependency initialization, a script executes its top-level statements as that module's initialization.
2. r[module.init.main] An executable package then invokes `main` after its entry module has initialized.
3. r[module.init.tests] Test runners initialize the module under test and the modules it uses before running its test cases.
4. r[module.init.test-cases] The `it` calls of a `tests:` block or a test module, and their bodies, are not part of module initialization.
5. r[module.init.script-empty] An entry module with no `main` and no top-level executable statements is a script with no entry behavior. Running it does nothing and exits with status 0.
6. r[module.init.tests.no-entry] A test run runs no entry behavior, so the module under test is not an entry module in it, even when it is a script or a [task](../cli/command-line.md#tasks).
7. r[module.init.tests.requirement-free] A script's top-level statements are then its module initialization, which must be requirement-free by [`module.init.requirement-free`](#r-module.init.requirement-free). Error: `missing-requirement`.

```text
# tasks/seed.hd, under `hd test`
use std.testing.assert_equal

fn rows() -> List[string]: ["apple", "pear"]

println("seeded ${rows().len()} rows")  # error: missing-requirement

tests:
    it("seeds two rows"):
        assert_equal(rows().len(), 2, reason="the seed rows")
```

> **Why.** A test run never runs `main`, and a script's top level plays
> the part of `main`. The runner binds no host for initialization, so a
> tested script keeps its work in `main`, as a Go or Rust program does.

### Requirement-Free Initialization

1. r[module.init.requirement-free] Top-level code in a non-entry module must be requirement-free initialization.
2. r[module.init.requirement-free.forms] It must not use `$.use`, enter a provider scope, make a bang call, or call a callable whose requirement row is not empty.
3. r[module.init.script-row] A script module may use requirements and suspension only through an inferred entry requirement row.
4. r[module.init.script-row.report] The compiler reports that row alongside `main!` rows for host configuration.
5. r[module.init.script-not-driver] A script's top level is not itself a suspension driver. Bang calls must occur in a suspending entry function or another specified driver context.

### Program Instances

1. r[module.init.per-instance] This initialization rule governs one program instance.
2. r[module.init.histories] Interactive cell re-execution and durable replay have separate runtime histories described in `RUNTIME_AND_LIBRARY.md`.

## Public Uses And Visibility

Declarations are module-private by default, and prefix `pub` makes a
declaration available to other modules:

```text
pub type UserId(string)

pub data User:
    pub id: UserId
    pub email: string
```

1. r[module.vis.private-default] Declarations are module-private by default.
2. r[module.vis.pub] Prefix `pub` makes a declaration available to other modules.

### Public Uses

Use `pub use` in a `mod.hd` file to expose a package-facing API:

```text
pub use pkg.user.types.{User, UserId}
pub use pkg.user.service.{load_user, save_user}
```

Another module can then use the names through the directory module:

```text
use pkg.user.{User, UserId, load_user}
```

1. r[module.pub-use.facade] `pub use` in a `mod.hd` file exposes a package-facing API, whose names another module can use through the directory module.
2. r[module.pub-use.public-source] A publicly used declaration must already be public in its defining module.
3. r[module.pub-use.binding] `pub use` introduces the same local binding as `use` and additionally exposes that binding to other modules.
4. r[module.pub-use.identity] `pub use` does not create a new declaration identity.
5. r[module.pub-use.chain] A `pub use` chain must end at a declaration: following each `pub use` of a name to the module it names must reach the module that declares the name.
6. r[module.pub-use.chain.loop] A chain that returns to a `pub use` it has already passed is an error. Error: `re-export-loop`.
7. r[module.pub-use.chain.loop-use] A plain `use` whose name leads into such a loop is an error with the same code. Error: `re-export-loop`.

```text
# src/shop/a.hd
pub use pkg.shop.b.{Token}  # error: re-export-loop

# src/shop/b.hd
pub use pkg.shop.a.{Token}

# src/main.hd
use pkg.shop.a.{Token}  # error: re-export-loop
```

> **Why.** Loops of `use` lines are allowed inside a folder, so a facade and
> its children may use each other. A name still needs one declaration.

### Member Visibility

1. r[module.vis.no-package-private] There is no package-private visibility modifier: another module in the same package can use only `pub` declarations.
2. r[module.vis.members] Named fields and inherent methods are module-private unless individually marked `pub`.
3. r[module.vis.variants] Enum variants inherit their enum's visibility.
4. r[module.vis.trait-methods] Trait methods follow their trait's visibility.
5. r[module.vis.impl-target] A usable implementation additionally requires its target type to be visible.

### Public Signatures

1. r[module.vis.signature] A public declaration's complete source-level signature must not expose a module-private declaration. Error: `private-type-leak`.
2. r[module.vis.signature.coverage] This check recursively covers function parameters and results, data fields (every embedded field included), and enum constructor data and payloads. It also covers alias/newtype underlying types, trait bounds, supertraits, public generic arguments, and every requirement-row key.
3. r[module.vis.body] A private implementation detail may occur in a public function body but not in its public typed interface.

```text
data Secret:
    value: string

pub fn reveal() -> Secret:  # error: private-type-leak
    Secret { value: "hidden" }
```

See also: [Field Visibility](08-data-and-enums.md#field-visibility).

## Name Resolution Across Packages

1. r[module.package.identity] An absolute qualified name identifies one declaration after dependency resolution.
2. r[module.package.no-sharing] Two dependencies do not share declarations merely because their package names and source paths match.
3. r[module.package.identity-part] Resolved package identity is part of a declaration's identity.
4. r[module.package.public-surface] A package must not access another package except through declarations reachable from that package's public module surface.
5. r[module.package.separate] Implementations may compile packages separately.

### Fully Annotated Declarations

1. r[module.package.annotated] Every public declaration is fully annotated: its complete signature is written in source.
2. r[module.package.annotated.parts] That signature includes parameter and result types, requirement rows, suspension, and generic parameters with their bounds and variance. It also includes the types of public fields and enum data.
3. r[module.package.no-inference] Nothing in a public signature is inferred from a function body.
4. r[module.package.no-pub-binding] Top-level bindings cannot be public. A `pub` binding is an error. Error: `syntax-error`.

```text
pub answer := 42  # error: syntax-error
```

### Package Interfaces

A package interface must contain:

| Interface content |
| --- |
| exported declaration identities and complete signatures |
| visibility |
| generic kinds, variance, and bounds |
| requirement rows |
| associated types |
| every ordinary and local implementation head needed for coherence |
| the bodies of pack code, which downstream compilation specializes |

1. r[module.interface.contents] A package interface must contain every item in the table.
2. r[module.interface.generic-bodies] An interface may also carry ordinary generic bodies to enable inlining, but downstream compilation must not require them.
3. r[module.interface.dictionaries] An implementation compiles each ordinary generic function in its defining package, and a downstream use supplies only its dictionaries.
4. r[module.interface.fact-values] A package interface records each [fact](14-annotations.md#r-annot.fact.eval) of its declarations by the fact's value.
5. r[module.interface.determined-facts] A package interface is therefore determined by the package's declarations and their fact values. It depends on no function body except through those values.
6. r[module.interface.early-facts] A downstream package can be compiled as soon as the interfaces of its dependencies are known. It need not wait for their function bodies to be checked or compiled, except the bodies their fact expressions call.
7. r[module.interface.coherence] Coherence is checked at link time over the complete set of resolved interface files.
8. r[module.interface.link-reject] Linking may therefore reject a graph even when each package compiled independently.

## Executable Entry Point

The conventional non-suspending entry point is:

```text
pub fn main() -> void:
    ...
```

1. r[module.entry.definition] An **executable entry point** is a public top-level function named `main` or `main!` with no parameters.
2. r[module.entry.result-termination] Its result type must implement `std.process.Termination`, as for an ordinary trait bound, so it may be `void`, `ExitCode`, or `Result[T, E]` with `T < Termination` and `E < Display`. Any other result type is an error. Error: `unsatisfied-trait-bound`.
3. r[module.entry.row] It may declare a requirement row.
4. r[module.entry.row.host] Every key in that row must be a host capability trait of the selected runtime profile. Any other key is an error. Error: `nonhost-entry-requirement`.
5. r[module.entry.private-main] A top-level `main` that is not public is an ordinary function and is not an entry point.
6. r[module.entry.suspending] A suspending entry point is spelled `main!`.
7. r[module.entry.private-main.warn] A top-level `main` or `main!` that is not public in an entry module gets a warning whose message is "main is not pub, so it is not the entry point". Warning: `private-main`.

```text
fn main() -> void:  # warning: private-main
    pass
```

> **Why.** An agent that drops `pub` from `main` gets a program that does
> nothing, with no error. The warning names the cause.

See also: [Mutable Providers](11-requirements-and-suspension.md#mutable-providers).

### Entry Results

1. r[module.entry.exit-report] When an entry point returns, the process exits with the `u8` held by the `ExitCode` that `report()` returns for its result, as [Exit Status](#exit-status) describes.
2. r[module.entry.err-host-prints] When the result holds an `.Err`, the host prints that error by the rules below before it exits.
3. r[module.entry.err-dynamic] A dynamic trait value type whose trait is `Display` or has it as a supertrait, such as the erased `std.error.Error`, satisfies the `E < Display` bound.
4. r[module.entry.err-render-chain] When `E` implements `std.error.Error`, including the erased `Error`, the host prints the error's `Display` text and then each cause that the standard-library `chain` yields after it.
5. r[module.entry.err-render-chain.line] Each cause is printed on its own line as `caused by: ` followed by the cause's `Display` text.
6. r[module.entry.err-render-display] Otherwise the host renders the error with `Display.to_string`.
7. r[module.entry.panic] A panic exits with a distinct nonzero status selected by the runtime profile and poisons the program instance.

```text
pub data HiddenError: pass

pub fn main() -> Result[void, HiddenError]:  # error: unsatisfied-trait-bound
    .Err(HiddenError {})
```

See also: [Dynamic Trait Values](09-traits.md#dynamic-trait-values),
[Error Trait](09-traits.md#error-trait),
[Cause Chain](../std/error.md#cause-chain).

#### Exit Status

`std.process` declares the exit code and the trait that produces it:

```text
pub type ExitCode(u8)

pub trait Termination:
    fn report(self) -> ExitCode
```

1. r[module.entry.exit-code] `std.process` declares the newtype `ExitCode`, whose `u8` is a process exit code. Every `u8` is valid, and 0 means success.
2. r[module.entry.termination] `std.process` declares the trait `Termination`, whose one method is `fn report(self) -> ExitCode`.
3. r[module.entry.termination.void] `void` implements `Termination`; its `report` returns `ExitCode(0)`.
4. r[module.entry.termination.exit-code] `ExitCode` implements `Termination`; its `report` returns the code itself.
5. r[module.entry.termination.result] `Result[T, E]` implements `Termination` when `T < Termination` and `E < Display`.
6. r[module.entry.termination.ok] For `.Ok(value)`, its `report` returns `value.report()`.
7. r[module.entry.termination.err-code] For `.Err(error)`, its `report` returns `ExitCode(1)`.
8. r[module.entry.termination.no-print] `report` only computes the code. The host or test runner prints the error, as [Entry Results](#entry-results) describes.
9. r[module.entry.process-import] `ExitCode` and `Termination` are not prelude names; code imports them from `std.process`, as in `use std.process.ExitCode`.

A program that picks its own code returns an `ExitCode`:

```text
use std.process.ExitCode

fn count_changes() -> Result[i32, string]:
    .Ok(3)

pub fn main() -> Result[ExitCode, string]:
    changes := count_changes()?
    if changes > 0: .Ok(ExitCode(1)) else: .Ok(ExitCode(0))
```

> **Why.** One trait serves entry points and tests, as Rust's `Termination`
> does, and the result type, not the error type, picks the code.

> **Note.** `main() -> Result[void, E]` therefore exits with 1 on every
> `.Err`. A tool that needs other codes returns `ExitCode` or
> `Result[ExitCode, E]`.

> **Note.** A tool that must exit without printing, such as one that stops
> quietly on a closed pipe, prints what it needs and returns an `ExitCode`.

### Entry Arguments

1. r[module.entry.no-arguments] `main` has no source-level arguments.
2. r[module.entry.host-facilities] Process arguments, console access, environment, and other host facilities are requirements supplied by the runtime through the requirement model.

## Wasm Boundary

This section defines runtime profiles, registered boundary functions, the
values that cross a boundary, and the official host boundary.

### Runtime Profiles

1. r[module.profile.definition] A **runtime profile** is a named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses.
2. r[module.profile.trait-access] The profile binds each host provider with the access its trait gives: mutable when the trait has a `mut self` method, readonly otherwise.
3. r[module.profile.build] The compiler receives the selected profile as build configuration.
4. r[module.profile.default] The default profile contains at least the prelude `Console` trait.
5. r[module.profile.other] Another profile may add or omit host traits explicitly.
6. r[module.profile.toolchain-names] Profile names are defined by the toolchain. A manifest cannot define a profile.
7. r[module.profile.panic-status] A profile's panic exit status is never 101.

> **Why.** `hd` exits with 101 when it fails itself
> ([`cli.exit.hd-failure`](../cli/command-line.md#r-cli.exit.hd-failure)),
> so a panicking program and a failed build never share a status.

See also: [Mutable Providers](11-requirements-and-suspension.md#mutable-providers).

### Host Results

A profile's boundary adapter checks every value that comes in from the
host before hd code sees it:

1. r[module.profile.host-result.checked] The adapter checks each value that a call of a host capability trait's method returns, suspending or not, against the result type that the method declares.

| Rule | Declared type | The value must be |
| --- | --- | --- |
| r[module.profile.host-result.integer] Integer | an integer type | an integer within the type's range |
| r[module.profile.host-result.bool] `bool` | `bool` | `false` or `true` |
| r[module.profile.host-result.char] `char` | `char` | a Unicode scalar value |
| r[module.profile.host-result.string] `string` | `string` | valid UTF-8 text |
| r[module.profile.host-result.float] Float | `f32` or `f64` | any value of that width, so a NaN, an infinity, or `-0.0` is never a contract violation |
| r[module.profile.host-result.shape] Composite | an optional, `Result`, tuple, list, map, data type, or enum | of that type's shape, with each element, field, and payload checked against its own declared type |

2. r[module.profile.host-result.no-coercion] The adapter never rounds, truncates, wraps, or otherwise converts a host value to make it fit.
3. r[module.profile.host-result.contract-panic] A value that fails the check is a host fault. The call panics with a message that names the trait and the method and says that the host broke its contract. Panic: `host-contract`.
4. r[module.profile.host-result.status] Like every panic, it poisons the program instance, and an entry point exits with the profile's panic exit status, by [`module.entry.panic`](#r-module.entry.panic).

> **Why.** A host that returns `300` for a `u8` has a bug outside hd.
> Stopping at the boundary names the host, where a silently wrapped value
> would surface later as a wrong answer in hd code.

#### Floats At The Host Boundary

A float crosses a live host call as its IEEE 754 value, with no encoding:

1. r[module.profile.host-float.raw] A call of a host capability trait's method passes each `f32` or `f64` argument, and returns each such result, as a raw IEEE 754 value of that width.
2. r[module.profile.host-float.special] So a NaN, both infinities, and `-0.0` cross the call unchanged, in either direction.
3. r[module.profile.host-float.nan] A NaN's payload may arrive replaced with the canonical NaN of [`types.display.nan-canonical`](04-type-system.md#r-types.display.nan-canonical).

> **Why.** WIT's `f64`, wasm-bindgen, and the C ABI all pass floats as raw
> IEEE 754 values. Special values need care only when a value becomes text,
> which [`module.boundary.float-bits`](#r-module.boundary.float-bits) covers.

### Registration

1. r[module.register.pub] `pub` is module visibility, not Wasm export registration.
2. r[module.register.explicit] A tool, workflow, or other host-callable function becomes visible only through explicit registration provided by its library.
3. r[module.register.entry] Every registered boundary function is an entry point for provider checking.
4. r[module.register.contract-traits] Its registration contract selects a runtime profile and declares which requirement traits that profile can bind.
5. r[module.register.application] That bindable set may include application traits such as `Database` when the adapter explicitly supports them.
6. r[module.register.row] Every key in the registered function's row must be in that bindable set, and the host must bind all of them before invocation.
7. r[module.register.failure] Otherwise registration or startup fails before user code executes.

### Boundary-Safe Values

Registered boundaries initially allow recursively structural values:

- primitive scalar types and `string`;
- tuples;
- `List[T]` and `Map[K, V]` whose contents are boundary-safe;
- data types and enums whose complete fields and payloads are boundary-safe;
- `T?` and `Result[T, E]` whose contained types are boundary-safe.

1. r[module.boundary.allowed] Registered boundaries initially allow the recursively structural values in the list above.
2. r[module.boundary.not-safe] Mutable types, dynamic trait values (including `Inspectable` values), `std.inspect.TypeId`, closures, and live runtime handles are not boundary-safe.
3. r[module.boundary.rows] Requirement-row entries are host bindings and are not serialized parameters.
4. r[module.boundary.erased-error] In particular, the erased error `std.error.Error` never crosses a registered boundary.
5. r[module.boundary.domain-error] A registered function returns a boundary-safe error type, such as a domain error enum, and code converts an erased error to such a type explicitly.

> **Note.** An error type with a member of type `Error` is therefore not
> boundary-safe either. Before such an error crosses a boundary, code
> converts it with the standard-library `report_of` to an `ErrorReport`,
> a boundary-safe snapshot of its message and causes.

See also: [Error Trait](09-traits.md#error-trait).

### Boundary Encoding

1. r[module.boundary.tree] Boundary values have tree semantics.
2. r[module.boundary.cycle] Encoding a cycle is a boundary error. Boundary failure: `boundary-cycle`.
3. r[module.boundary.sharing] When an acyclic graph shares a node, each incoming path encodes a duplicate tree value, and decoding does not restore sharing.
4. r[module.boundary.map-decode] Decoding a map invokes the key type's ordinary `Eq` and `Hash` implementations.
5. r[module.boundary.decoder-panic] If either panics, the adapter reports a boundary failure and does not enter the registered function. Boundary failure: `boundary-decoder-panic`.
6. r[module.boundary.decoder-poison] The adapter treats that event as an ordinary poisoning panic: the program instance must be discarded.
7. r[module.boundary.float-bits] A boundary value or host-call result serialized as text, as in a replay record, writes an `f64` as its IEEE 754 bit pattern. That is 16 lowercase hex digits, most significant first.
8. r[module.boundary.float-bits.f32] An `f32` is written the same way, as 8 lowercase hex digits.
9. r[module.boundary.float-bits.nan] A NaN is first replaced with the canonical NaN of [`types.display.nan-canonical`](04-type-system.md#r-types.display.nan-canonical).

| Value | `f64` text |
| --- | --- |
| `1.0` | `3ff0000000000000` |
| `-0.0` | `8000000000000000` |
| positive infinity | `7ff0000000000000` |

> **Why.** A float's bits make its text exact, while a NaN, an infinity,
> or `-0.0` has no JSON number.

> **Note.** The reference prototype's replay encoder writes host-call
> results this way.

#### Private Fields At A Boundary

A data type with a private field crosses a boundary only with its
author's [serialization consent](14-annotations.md#serialization):

```text
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize)
pub data Token:
    pub owner: string
    secret: i64

pub data Badge:
    pub owner: string
    code: i64

pub trait Vault:  # a host capability of the selected runtime profile
    fn keep(self, token: Token) -> Token
    fn badge(self) -> Badge  # error: boundary-private-field
```

1. r[module.boundary.consent.out] A value of a data type with a field that is not `pub` crosses a boundary out of hd only when the type implements `std.serde.Serialize`.
2. r[module.boundary.consent.in] Such a value crosses a boundary into hd only when the type implements `std.serde.Deserialize`.
3. r[module.boundary.consent.public] A data type whose fields are all `pub` crosses in either direction without either trait. An enum's payloads have no visibility, so they need no consent.
4. r[module.boundary.consent.direction] A registered function's arguments cross into hd, and its result crosses out. A host capability method's arguments cross out of hd, and its result crosses in.
5. r[module.boundary.consent.error] A boundary signature whose type, or a type inside it, would cross without the consent its direction needs is an error, reported at that signature. Error: `boundary-private-field`.
6. r[module.boundary.consent.tree] A consented value crosses as the same tree of fields as any other data value. The consent permits the crossing and changes no encoding.

> **Why.** A host is one more format. A type that lets JSON read its
> private fields lets the host read them too. Without its author's
> consent, its private state stays away from both.

> **Note.** The standard library's `Duration`, `Timestamp`, and `Instant`
> consent both ways, so a `Clock` provider's results cross into hd
> ([Time](../std/time.md#serialization)). Whether a boundary should encode
> a value through its consent's methods is open, in
> [Serialization Formats](../../future-work/OPEN_ISSUES.md#serialization-formats).

### Instances And Threads

1. r[module.instance.thread] One program instance executes on one thread and has no shared-memory parallelism or source-level atomics.
2. r[module.instance.parallel] Hosts may run multiple Wasm instances in parallel only by exchanging boundary-safe values.
3. r[module.instance.disjoint] Their heaps, mutable globals, and suspension drivers are disjoint.

### Host Boundary

1. r[module.host.wasm] The official compiler targets Wasm only.
2. r[module.host.runtime] The official runtime uses Wasm GC for managed language values and a WASI-compatible host boundary.
3. r[module.host.providers] Authority-bearing providers originate at that boundary.
4. r[module.host.component-model] The only normative host boundary is the WebAssembly Component Model, and strings cross it as canonical-ABI strings.
5. r[module.host.tooling-hooks] Hooks that a toolchain adds for development, testing, or tracing are implementation tooling, not a language boundary.
6. r[module.host.requirements] Every host facility is injected through an ordinary requirement trait.
7. r[module.host.capability-set] The eventual standard capability-trait set is runtime and library work.
8. r[module.host.abi] The exact component-model ABI and registration APIs are runtime and library specification work.

### Closable Handles

Closable runtime handles use `std.resource.ResourceError[E]`:

```text
enum ResourceError[E]:
    Operation(error: E)
    Disposed
```

1. r[module.resource.error] Closable runtime handles use `std.resource.ResourceError[E]`.
2. r[module.resource.result] An operation whose ordinary error type is `E` returns `Result[T, ResourceError[E]]`.
3. r[module.resource.disposed] Once the handle has been closed, every further operation returns `.Err(ResourceError.Disposed)` and must not trap or access the host resource.
4. r[module.resource.close] Closing is itself an operation, and a repeated close returns the same `Disposed` error.
5. r[module.resource.aliases] This checked behavior applies through every alias.

## Tooling, ABI, And Unsupported Extensions

1. r[module.tooling.abi] The exact Wasm component boundary and registration mechanism belong to the runtime ABI.
2. r[module.unsupported.visibility] hd-lang has no package-private visibility or independent visibility for enum variants and trait methods.

See also: [Command Line](../cli/command-line.md), which defines package tooling: package
mode, executables, tasks, and the `hd` commands.
