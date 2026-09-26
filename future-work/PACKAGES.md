# Packages: Survey And Manifest Draft

Status: research and design draft for [Roadmap area 5](ROADMAP.md#5-packages).
Nothing here is accepted language or tooling behavior. Decisions that change
the language go to [Open Issues](OPEN_ISSUES.md) and the
[specification](../spec/README.md); this document proposes, it does not decide.

Inputs:

- [Package Manifest](../spec/10-modules.md#package-manifest),
  [Use Roots](../spec/10-modules.md#use-roots),
  [Name Resolution Across Packages](../spec/10-modules.md#name-resolution-across-packages)
  (interface files, link-time coherence), and
  [Executable Entry Point](../spec/10-modules.md#executable-entry-point).
- The orphan and overlap rules in [Traits](../spec/09-traits.md) and
  [Coherence And Package Rules](../spec/14-annotations.md#coherence-and-package-rules)
  for annotations, including the root-application orphan exception.
- The package bullet in
  [Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work).

Current state. The prototype in `src/` does not read `hd.toml`. The
conformance runner simulates a package graph instead: it passes
`--package-role library|root-application` and one `--dependency NAME=DIR` per
directory under [`spec/conformance/packages/`](../spec/conformance/packages),
and each such directory is a source root whose `mod.hd` is the package root
module ([Package Roles](../spec/conformance/README.md#package-roles)). The
draft below keeps that model: a manifest dependency key is the `NAME` in
`dep.NAME`, and a package kind is the role.

## 1. Survey

### 1.1 Comparison Table

| Topic | Cargo (Rust) | uv / `pyproject.toml` | Go modules | SwiftPM | Gradle (Kotlin) | MoonBit `moon` |
| --- | --- | --- | --- | --- | --- | --- |
| Manifest | `Cargo.toml`: `[package]`, `[lib]`, `[[bin]]`, `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, `[features]`, `[profile.*]`, `rust-version`, `edition` | `pyproject.toml`: `[project]` (PEP 621), `requires-python`, `[dependency-groups]` (PEP 735), `[tool.uv.sources]`, `[tool.uv.workspace]` | `go.mod`: `module`, `go`, `toolchain`, `require`, `replace`, `exclude`, `retract`, `tool` | `Package.swift`, executable Swift DSL: products, targets, dependencies, `swift-tools-version`, traits (SE-0450) | `build.gradle.kts` (executable Kotlin DSL), `settings.gradle.kts`, version catalog `libs.versions.toml` | `moon.mod.json` / new `moon.mod` DSL per module (name, version, deps, source, targets); `moon.pkg.json` / `moon.pkg` per package (import, test-import, pkgtype executable) |
| Constraint syntax | caret by default (`"1.2"` = `>=1.2.0,<2.0.0`), `~`, `=`, `*`, comparison ranges | PEP 440: `>=`, `~=`, `==`, `!=`, `===`, markers | minimum version only: `require x v2.3.1` means at least that | `from:` (up to next major), `.upToNextMinor`, `exact`, ranges | `1.2` (prefer), rich versions `strictly`/`require`/`prefer`/`reject`, dynamic `1.+` | exact version string read as minimum |
| Resolver | backtracking search with semver unification; MSRV-aware since resolver 3 | PubGrub fork, universal (all platforms and Python versions at once) | minimal version selection (MVS): pick the largest stated minimum, no search | PubGrub | conflict resolution picks the highest requested version unless constrained | MVS |
| Several versions of one package | yes, one per semver-compatible line (`1.x` and `2.x`; each `0.x` minor is its own line) | no, one per environment | one per major: `/v2` suffix makes a different module path | no, one per package identity | no, one per module per configuration | one per major since `/vN` module paths (merged September 2026) |
| Lockfile and hashing | `Cargo.lock` TOML, SHA-256 of each registry `.crate` | `uv.lock` TOML, SHA-256 per sdist and wheel, resolution markers; PEP 751 `pylock.toml` standard export | no version lock needed; `go.sum` holds `h1:` SHA-256 tree hashes, checked against the public checksum database | `Package.resolved` JSON pins (revision, version) plus `originHash`; checksums for registry and binary targets | `gradle.lockfile` versions only; hashes in separate `verification-metadata.xml` | no lockfile documented |
| Workspaces | `[workspace]` members, one shared lock and target dir, `workspace.dependencies` inheritance | `[tool.uv.workspace]` members, one shared `uv.lock` | `go.work`, local development only, not published | none in SwiftPM (local path deps; Xcode workspaces) | multi-project builds and composite builds | `moon.work`, local resolution overrides versions |
| Sources | registry (crates.io, alternative registries), git, path; `[patch]` override | index (PyPI or others), git, path, URL, workspace | any VCS through module proxy (`GOPROXY`); `replace` for path | git URL, path, registry (SE-0292) | Maven repositories, included builds | mooncakes.io registry, path |
| Standard library vs toolchain | std shipped with rustc; toolchain chosen by `rust-toolchain.toml`; `rust-version` declares the minimum | stdlib is the Python interpreter; `requires-python` constrains it, uv can install interpreters | std shipped with toolchain; `go` line is a minimum, `toolchain` line and `GOTOOLCHAIN` fetch newer toolchains automatically | stdlib and Foundation ship with toolchain; `swift-tools-version` gates the manifest API | `kotlin-stdlib` is an ordinary dependency, added by the Kotlin Gradle plugin at the plugin's version | `moonbitlang/core` bundled with the toolchain |
| Semver checking | `cargo-semver-checks` (rustdoc JSON lints); merge into `cargo publish` is a 2026 project goal, not done | no standard tool; `griffe check` compares Python APIs | `apidiff` (`golang.org/x/exp`), `gorelease` suggests the next version | `swift package diagnose-api-breaking-changes <ref>` | ABI dumps: `binary-compatibility-validator`, now built into the Kotlin Gradle plugin as `abiValidation` (experimental since 2.2) | none |

### 1.2 Notes Per Ecosystem

**Cargo.** Coexistence of semver-incompatible versions avoids most solver
failures but produces the familiar "expected `serde::Serialize`, found
`serde::Serialize`" error: two versions of one crate declare distinct traits,
and Rust's orphan rule prevents a third crate from bridging them. The
community workaround is the "semver trick": release `1.9` of a crate as a
re-export of `2.0`, so the old major's types become the new major's types.
Features are additive flags unified across the graph. They are one of Cargo's
largest sources of complexity and of surprise builds. Public and private
dependencies (RFC 3516) remain unstable. That leaves no checked difference
between a dependency used internally and one exposed in the API.

**uv.** Fast, and its lock is universal: one file covers every platform and
Python version by carrying environment markers. Python forces one version
per environment, so an ecosystem-wide major bump waits on every consumer.
`[dependency-groups]` gives named, non-published groups such as `dev` and
`test`, which is a cleaner shape than one fixed `dev` table.

**Go modules.** MVS is deterministic, needs no search, and selects the same
build from the manifests alone, so the lock only guards integrity. A build
changes only when some `go.mod` changes. The costs: no upper bounds, so a
known-bad version is avoided only with `exclude` in the main module; and the
scheme trusts that newer minors are compatible, which nothing checks.
Semantic import versioning (`/v2` paths) makes each major a distinct module,
so majors coexist and are visible in source. The checksum database
(`sum.golang.org`) gives every consumer the same hash for a version.
`retract` lets an author mark a published version as bad without deleting it.

**SwiftPM.** The manifest is executable Swift, which is expressive but cannot
be edited or queried reliably by tools without running it. Package identity
derived from the URL's last component has caused collisions. `Package.resolved`
pins git revisions but does not hash source content. Traits (SE-0450) add
Cargo-like optional features.

**Gradle.** Rich versions (`strictly`, `reject`) and highest-wins conflict
resolution make the build depend on every declaration in the graph. The lock
and the hash verification are separate files. Version catalogs show the value
of one declarative, tool-editable file for versions. The Kotlin ABI validator
checks a committed API dump in CI. That is the closest existing model to
checking compatibility from an interface file.

**MoonBit.** Closest in spirit to hd: small manifest, MVS, a toolchain-bundled
core library, and a per-package file that marks executables. The move from
JSON to a small DSL shows JSON was not pleasant to write. MoonBit recently
added `/vN` module paths so majors can coexist, following Go.

### 1.3 Takeaways For hd

1. **MVS fits hd best.** It is deterministic, explainable in one sentence
   ("the largest minimum anyone asked for"), and never picks a version that no
   manifest named. Its one weak assumption, that newer minors are compatible,
   is exactly what hd's interface files let the registry check at publish.
   Go and MoonBit trust authors; hd can verify them.
2. **Interface files replace rustdoc JSON or ABI dumps.** hd already requires
   complete public signatures and an interface file per package. A
   compatibility checker compares two interface files and needs no extra
   extraction step. None of the surveyed ecosystems start this well placed.
3. **Coexistence of majors interacts with coherence.** Rust shows the cost of
   coexistence without tooling (duplicate-looking traits). Python shows the
   cost of forbidding it (stalled upgrades). Go and now MoonBit take the
   middle road: one version per major, with the major part of identity.
   hd's `pub use` keeps declaration identity, so the semver trick works
   without extra machinery.
4. **Declarative, non-executable manifest.** SwiftPM and Gradle manifests are
   programs; agents and query tools cannot edit them safely. `hd.toml` stays
   plain TOML with a closed schema, and unknown keys are errors.
5. **No optional features.** Cargo features and Swift traits add build
   variants and graph-wide unification. hd has no conditional compilation;
   keeping one build per package version keeps interface files unique.
6. **Lock integrity by content hash, over a canonical file tree.** Go's `h1:`
   tree hash is independent of archive format. uv and Cargo hash archives.
   Hashing the canonical tree lets git, path, and registry sources share one
   check.
7. **Standard library tied to the toolchain.** Every surveyed language except
   Kotlin does this. The manifest states a minimum toolchain, like Go's `go`
   line.
8. **Separate dependency groups for tests.** hd's use roots are module-wide,
   so test-only dependencies need a module-level boundary, like MoonBit's
   `test-import` and Go's `_test.go` files.

## 2. Draft: `hd.toml`

### 2.1 Design Goals

- One package is one source root, one kind, one version, and one interface
  file.
- The manifest is plain TOML with a closed schema. Every key has one meaning.
  Unknown keys, and keys that are illegal for the package kind, are errors.
- Tools edit the manifest; humans read it. `hd` rewrites it in a canonical
  key order, so an agent's edit produces a minimal diff.
- Nothing in the manifest duplicates what source already states. Provider
  bindings are derived from entry points
  ([Runtime and Library Design](RUNTIME_AND_LIBRARY.md)), and public API is
  derived from `pub` declarations.

### 2.2 Full Example: Application

```toml
[package]
name = "invoice_cli"
kind = "application"
version = "0.3.0"
hd = "0.9.0"
description = "Command-line invoice generator"
license = "MIT"
repository = "https://github.com/acme/invoice-cli"

[source]
root = "src"
tests = "tests"

[[executable]]
name = "invoice"
module = "main"
profile = "console"

[[executable]]
name = "invoice-migrate"
module = "tools.migrate"
profile = "console-fs"

[dependencies]
json = "acme/json@2.1.0"
billing = "acme/billing@1.4.2"
billing_legacy = "acme/billing@0.9.3"
shared = { path = "../shared" }
pdf = { git = "https://github.com/acme/hd-pdf", rev = "3f2c9e1a7b6d4c58e0f1a2b3c4d5e6f708192a3b" }

[test-dependencies]
fixtures = "acme/test_fixtures@0.2.0"

[build.release]
optimize = "speed"

[patch]
"acme/json" = { path = "../json" }
```

### 2.3 Full Example: Library

```toml
[package]
name = "billing"
id = "acme/billing"
kind = "library"
version = "1.4.2"
hd = "0.9.0"
description = "Billing primitives"
license = "Apache-2.0"
repository = "https://github.com/acme/billing"

[source]
root = "src"
tests = "tests"

[dependencies]
money = "acme/money@3.0.1"

[test-dependencies]
fixtures = "acme/test_fixtures@0.2.0"
```

### 2.4 Full Example: Workspace Root

```toml
[workspace]
members = ["apps/invoice_cli", "libs/billing", "libs/shared"]
```

A workspace root has no `[package]` table. Members are ordinary packages.
There is one resolution and one `hd.lock` for the whole workspace, at its
root. A member depends on another member with `{ path = "..." }`.

### 2.5 Schema

`[package]`, required in every package:

| Key | Type | Rule |
| --- | --- | --- |
| `name` | identifier | The package's local name, used in diagnostics and as the default executable name. Source never names it: the current package is always `pkg`. |
| `id` | `owner/name` | Registry identity. Required to publish; forbidden for applications. `name` must equal its last component. |
| `kind` | `"library"` or `"application"` | Selects the package role. Only an application may declare `[[executable]]` or be the root that uses the orphan annotation exception. Only a library may be a dependency. |
| `version` | `MAJOR.MINOR.PATCH[-PRE]` | See [Versions](#3-versions). Build metadata (`+...`) is rejected. |
| `hd` | version | Minimum toolchain version, which is also the minimum `std` version. |
| `description`, `license`, `repository`, `readme` | strings | Metadata. `license` is an SPDX expression. Required to publish. |

`[source]`, optional:

| Key | Default | Rule |
| --- | --- | --- |
| `root` | `"src"` | Source root for path-inferred modules ([Path-Inferred Modules](../spec/10-modules.md#path-inferred-modules)). `root/mod.hd`, when present, is the package root module and public index. |
| `tests` | `"tests"` if the directory exists | Test source root. Its modules are compiled only by `hd test`. They may use `pkg`, dependencies, and test dependencies, and they see only the package's public surface, as a downstream package would. |

The two roots must not overlap. Inline `test` blocks in `root` may use only
`[dependencies]`, because a `use` is module-wide and the module is also part
of the normal build.

`[[executable]]`, applications only, zero or more:

| Key | Default | Rule |
| --- | --- | --- |
| `name` | package `name` | Output artifact name. Unique within the package. |
| `module` | `"main"` | Entry module, as a module path relative to the source root. |
| `profile` | `"console"` | Runtime profile ([Wasm Boundary](../spec/10-modules.md#wasm-boundary)). The toolchain defines the profile names. |

If an application declares no `[[executable]]`, it has one implicit entry
with all defaults, so `src/main.hd` is the entry module. See
[Entry Points](#26-entry-points) below for the selection rule.

`[dependencies]` and `[test-dependencies]`: each key is an identifier and
becomes the `NAME` in `dep.NAME`. Each value is one of:

| Form | Meaning |
| --- | --- |
| `"owner/name@X.Y.Z"` | Registry package, minimum version `X.Y.Z`. |
| `{ id = "owner/name", version = "X.Y.Z", registry = "URL" }` | Registry package from a non-default registry. |
| `{ path = "DIR" }` | Local package. Not allowed in a published package. |
| `{ git = "URL", rev = "SHA" }` | Git package at one full commit hash. Branches and tags are rejected. Not allowed in a published package. |

A key may not name `std`, `pkg`, or `dep`. Two keys may name the same
registry package only at different majors; see [Coexistence](#43-coexistence).

`[build.<name>]`: named build configurations. `hd build` uses `dev` by
default and `--build release` selects `release`. Keys are limited to
optimization level and debug information. The table name is `build`, not
`profile`, to avoid a clash with runtime profiles.

`[patch]`: root only. It maps a registry `id` to a path or git source for the
whole graph. It is ignored, with a warning, in a package used as a
dependency. It is the only override mechanism.

`[workspace]`: workspace root only. `members` lists member directories.
Globs are rejected, so the member list is explicit in review.

### 2.6 Entry Points

The spec fixes which function is the entry point: a public top-level `main`
or `main!` with no parameters in the entry module. A non-public `main` is an
ordinary function. The manifest only selects the entry module:

1. Each `[[executable]]` names one entry module by `module`.
2. If the entry module declares a public `main` or `main!`, the executable
   invokes it after module initialization.
3. Otherwise, if the entry module has top-level executable statements, it is
   a script.
4. Otherwise the build fails with `missing-entry-point`, a tooling
   diagnostic that names the module and the declarations that almost
   qualified, such as a non-public `main` or a `main` with parameters.
5. A public `main` in a module that no executable selects is an ordinary
   public function. `hd` warns (`unselected-main`) so an agent notices a
   missing `[[executable]]` entry.
6. A library has no executables. `hd run` on a library fails with a
   diagnostic that names the package kind.

The manifest never names the function. That keeps one source of truth: an
agent that renames `main` sees a checker error in source, not a stale
manifest.

## 3. Versions

### 3.1 Syntax

A version is `MAJOR.MINOR.PATCH` with an optional `-PRE` pre-release suffix
using SemVer 2.0.0 ordering. All three numeric parts are required in the
manifest, the lockfile, and CLI output. Build metadata is rejected.

A dependency requirement is a single version, read as a minimum within its
compatibility line. There are no ranges, no upper bounds, no `!=`, and no
wildcards. A pre-release is selected only when some manifest names that exact
pre-release.

### 3.2 Compatibility Lines

The compatibility line of a version is its major number when the major is at
least 1, and `0.MINOR` when the major is 0. Two versions in one line must be
compatible under the checked rule below. Versions in different lines are
different packages for resolution and for declaration identity.

### 3.3 The Checked Compatibility Rule

Definition. Version `N` is compatible with an earlier version `O` of the
same line when every downstream package that type-checks against `O`'s
interface file, in a coherent resolved graph, still type-checks against
`N`'s interface file and still links coherently.

Tools cannot decide that property in general, so `hd api diff O N`
classifies each difference between the two interface files. Any
unclassified difference counts as breaking. The required bump is:

- **patch** when the two interfaces are signature-equal. Bodies of pack and
  reified code may differ; they are behavior, not signature.
- **minor** when every difference is an addition classified as compatible.
- **major** (a new line) otherwise.

Starting classification, to be revised as area 2 fixes method resolution and
coherence rules:

| Change | Class | Reason |
| --- | --- | --- |
| Add a public declaration with a new name | compatible | Wildcard uses do not exist, so a new name cannot collide with a downstream binding. |
| Remove a public declaration, field, method, variant, or implementation | breaking | Downstream references fail. |
| Make a public declaration non-public | breaking | Same as removal. |
| Change any part of a public signature: parameter or result types, generic parameters, bounds, variance, reification | breaking | Unless a later refinement proves a specific change safe, such as loosening a function's bound. |
| Add a key to a function's requirement row | breaking | Callers must now provide it. |
| Remove a key from a function's requirement row | compatible | Callers provide a superset. |
| Change a trait method's requirement row | breaking | Implementors and callers both depend on it. |
| Change `fn` to `fn!` or the reverse | breaking | Suspension changes how every call is written. |
| Add a public data field with a default | compatible | Construction may omit it. Needs confirmation against the data-pattern rules. |
| Add a public data field without a default | breaking | Existing constructions fail. |
| Add an enum variant | breaking | `match` is exhaustive and hd has no marker for open enums. |
| Add a trait method with a default | compatible | Subject to area 2's default-method conflict rules. |
| Add a trait method without a default | breaking | Existing implementations fail. |
| Add an inherent method | compatible, pending area 2 | Safe only if inherent lookup cannot change which method an existing call selects. |
| Add an implementation whose trait and target the package both own | compatible | No other package can hold that slot. |
| Add a generic (blanket) implementation | breaking | It can overlap an implementation in a downstream package. |
| Add an implementation or annotation whose trait or facet another package owns | owner question 6 | It can take a slot that a root application filled with an orphan annotation. |

The registry runs `hd api diff` against the highest published version of the
same line below the new one, and refuses a publish whose declared version is
smaller than the required bump. A larger bump is always allowed. The first
release of a line is unchecked.

For `std`, the same rule applies across toolchain releases. One extra rule
applies: adding a prelude name is breaking, because any downstream
declaration of that name becomes a `prelude-name-shadow` error.

## 4. Resolution

### 4.1 Algorithm

Minimal version selection, as in Go and MoonBit:

1. Start from the root package, or from every member of a workspace.
2. For each requirement, visit that package's manifest at the required
   version, collecting requirements transitively. `[test-dependencies]` of
   non-root packages are not visited.
3. For each package identity and compatibility line, select the largest
   minimum any visited manifest states. This is the whole algorithm. It needs
   no search and never fails for version reasons.
4. Apply root `[patch]` entries.
5. Check the graph: no cycles, only libraries as dependencies, each
   package's `hd` minimum at most the selected toolchain.
6. Load the interface file of every selected package and run the link-time
   coherence check (next section). A failure here is a resolution failure.

Because the registry checks compatibility, step 3's assumption, that the
largest minimum satisfies every smaller one, is verified rather than
trusted.

Selection changes only when some manifest changes. `hd update NAME` raises
the root's stated minimum, and the lockfile records the result.

A yanked version is never chosen as a new minimum by `hd add` or
`hd update`. It stays usable when a manifest already names it, and `hd`
warns, as Go does with `retract`.

### 4.2 Coherence Across Packages

The link-time check runs over the complete set of resolved interface files
([Name Resolution Across Packages](../spec/10-modules.md#name-resolution-across-packages)):

- At most one implementation per instantiated trait and target, with overlap
  checked across packages ([Traits](../spec/09-traits.md)).
- At most one annotation per `(facet, target)` slot, and only the root
  application may hold an orphan slot
  ([Coherence And Package Rules](../spec/14-annotations.md#coherence-and-package-rules)).

Because interface files contain every implementation head, `hd resolve`
runs this check before compiling any function body. A registry that serves
interface files lets `hd resolve` report a conflict before sources are
downloaded.

MVS makes the annotation rule predictable. The spec says that if a dependency
version later supplies a pair the root annotated, resolution fails. Under
MVS a dependency version changes only when a manifest does. So the failure
appears on the `hd add` or `hd update` that raised the version, and names
both annotations. It never appears on a later unrelated build.

A library is never the root, so it cannot contain an orphan annotation. That
holds when the library is built or tested alone, too: `hd build` and
`hd test` on a library use the library role, matching the conformance
runner's `library` role.

### 4.3 Coexistence

Draft rule: at most one version per compatibility line of a package. Two
lines of one package may coexist in a graph, and they are distinct package
identities. This is Go's and MoonBit's rule, with the line part of identity
instead of a `/vN` path.

Consequences under hd's rules:

1. Declaration identity includes the line. `acme/json@1` and `acme/json@2`
   declare different `Value` types and different `Serialize` traits.
2. Orphan rules use the same identity. A third package cannot implement
   `json@2`'s trait for a type owned by `models`. Only `models` or `json@2`
   can. `models` may depend on both lines at once and implement both traits,
   which supports a gradual migration. A one-version-only rule would make
   that impossible.
3. The semver trick works without extra machinery. `json@1.9.0` can depend on
   `json@2` and `pub use` its types, because `pub use` keeps declaration
   identity ([Public Uses And Visibility](../spec/10-modules.md#public-uses-and-visibility)).
   Downstream code on either line then sees one type.
4. A package can name two lines only through two dependency keys, as
   `billing` and `billing_legacy` do in the example. The source then shows
   which line each use refers to.
5. Diagnostics that mention a declaration from a package present at two lines
   must print the line, such as `dep.json.Value (acme/json@2)`. This rules out
   Rust's "expected `Serialize`, found `Serialize`" message.
6. `std` exists exactly once in every graph.

Owner question 1 covers the alternatives.

## 5. Lockfile

`hd.lock` sits next to the root manifest, or the workspace manifest, and is
committed. The lockfile of a dependency is ignored. Under MVS the manifests
alone determine selection, so the lockfile's job is integrity and a fast,
offline check. `hd build --locked` fails if resolution would produce a
different lockfile.

Format: TOML written by `hd` only, with packages sorted by `id` then version,
and keys in a fixed order. Every hash is written as `algorithm:hex` so the
algorithm can change later.

```toml
# Written by hd. Do not edit.
format = 1
toolchain = "0.9.4"
std = "sha256:4b1e9f0c2d3a5b6c7d8e9f00112233445566778899aabbccddeeff0011223344"

[[root]]
name = "invoice_cli"
path = "."
requires = ["acme/billing@0.9.3", "acme/billing@1.4.2", "acme/json@2.1.0", "pdf", "shared"]

[[package]]
id = "acme/billing"
version = "0.9.3"
source = "registry+https://packages.hd-lang.org"
content = "sha256:0c5d2f1e7a9b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5"
interface = "sha256:7d1a3b5c9e2f4a6b8c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b"
requires = ["acme/money@2.3.0"]

[[package]]
id = "acme/billing"
version = "1.4.2"
source = "registry+https://packages.hd-lang.org"
content = "sha256:9a8b7c6d5e4f30211a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081"
interface = "sha256:1f2e3d4c5b6a79880a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6071"
requires = ["acme/money@3.0.1"]

[[package]]
id = "acme/json"
version = "2.1.0"
source = "path+../json"
patched = true
interface = "sha256:2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f80910"
requires = []

[[package]]
id = "acme/money"
version = "2.3.0"
source = "registry+https://packages.hd-lang.org"
content = "sha256:3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b"
interface = "sha256:4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c"
requires = []

[[package]]
id = "acme/money"
version = "3.0.1"
source = "registry+https://packages.hd-lang.org"
content = "sha256:5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d"
interface = "sha256:6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e"
requires = []

[[package]]
name = "pdf"
version = "0.4.0"
source = "git+https://github.com/acme/hd-pdf?rev=3f2c9e1a7b6d4c58e0f1a2b3c4d5e6f708192a3b"
content = "sha256:708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f"
interface = "sha256:8192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70"
requires = []

[[package]]
name = "shared"
version = "0.1.0"
source = "path+../shared"
interface = "sha256:92a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081"
requires = ["acme/json@2.1.0"]
```

Hashes:

- `content` is a tree hash, as in Go: SHA-256 over the sorted lines
  `sha256(file) path` for every file in the published package, with `/`
  separators and NFC paths. It is the same for a registry archive, a git
  checkout, and an unpacked directory with the same files. Path sources have
  no `content` entry, because they are expected to change.
- `interface` is the SHA-256 of the canonical interface file. It lets a build
  cache skip downstream recompilation when a dependency's interface did not
  change, and it lets `hd resolve` detect an interface that differs from what
  the registry recorded.
- `std` is the interface hash of the toolchain's standard library.

Reproducibility: a build is determined by the lockfile, the toolchain
version, the build configuration name, and the source tree. Output Wasm must
be byte-identical for equal inputs. Environment variables, timestamps, and
absolute paths must not reach the output.

## 6. Standard Library

- `std` ships inside the toolchain and is never downloaded or listed as a
  dependency. Its version is the toolchain version.
- `[package] hd` is a minimum. The selected toolchain must be at least the
  largest `hd` value in the resolved graph, as with Go's `go` line under MVS.
- A toolchain pin (`hd-toolchain.toml` or `[toolchain]` in the root) is owner
  question 10.
- `std` follows the checked compatibility rule across toolchain releases in
  one line, with the prelude rule from [section 3.3](#33-the-checked-compatibility-rule).
  A breaking `std` change needs a new toolchain major.
- The toolchain version recorded in `hd.lock` is informative. `--locked`
  fails only if the selected toolchain is below the graph's minimum.

## 7. Distribution

A published package is a canonical archive containing:

- `hd.toml`, normalized: keys canonical, `[patch]` and `[workspace]`
  removed, path and git dependencies rejected before publishing;
- the source root and the test root;
- the interface file, regenerated and checked by the registry;
- `README` and license files.

It contains no compiled Wasm. Packages build from source, so there is one
compiler-owned representation of generic code. Shipping applications as Wasm
components is a runtime and deployment concern, separate from the registry
(owner question 11).

Registry rules:

- Package identity is `owner/name`. `owner` is a verified account or
  organization (owner question 3).
- Published versions are immutable. Yanking marks a version as not
  selectable for new requirements; it does not delete it.
- At publish, the registry recompiles the package's declarations, compares
  the interface hash with the uploaded one, runs `hd api diff` against the
  previous version in the line, and runs the link-time coherence check
  against the package's own minimum dependency graph.
- The registry serves each version's manifest and interface file separately
  from its archive, so resolution and coherence checks need no source
  download.

## 8. Agent-First CLI

Every package operation is a non-interactive command. No command prompts.
Anything that would need confirmation fails with a diagnostic that names the
flag to pass.

| Command | Effect |
| --- | --- |
| `hd new --kind library\|application NAME` | Create a package skeleton. |
| `hd add NAME ID@VERSION [--test]` | Add or raise a dependency, re-resolve, and update `hd.lock`. Without a version, use the newest non-yanked release. |
| `hd remove NAME` | Remove a dependency. |
| `hd update [NAME]` | Raise stated minimums to the newest compatible release. `--line` allows moving to a new line. |
| `hd resolve [--explain ID]` | Resolve and check coherence. `--explain` lists each requirer of the package and its stated minimum; under MVS that is the full explanation. |
| `hd lock --check` | Fail if `hd.lock` is stale. |
| `hd api diff [OLD] [NEW]` | Compare interface files and report the required bump. Defaults: last published version and the working tree. |
| `hd publish [--dry-run]` | Run every registry check locally, then upload. |
| `hd build`, `hd run [EXECUTABLE]`, `hd test` | Build, run, or test using the manifest. |
| `hd metadata` | Print the resolved package graph, executables, and interface hashes. |

Output rules:

- `--format json` is accepted by every command. JSON output is one document
  on stdout with a `schema` field naming its shape, such as
  `"hd.resolve/1"`. Human text is the default.
- Diagnostics use stable codes, such as `missing-entry-point`,
  `coherence-conflict`, and `version-bump-required`. Each JSON diagnostic
  carries the manifest or source span and, when one exists, a suggested edit
  as a replacement span.
- Exit codes are stable: 0 success, 1 diagnostics reported, 2 invalid
  invocation, 3 network or registry failure.
- `--plan` on any command that writes files prints the manifest and lockfile
  edits as JSON and changes nothing.
- `hd add`, `hd remove`, and `hd update` rewrite `hd.toml` in canonical form.
  Comments are kept.
- Package metadata is exposed to the program database from
  [Roadmap area 8](ROADMAP.md#8-tooling-for-agents): packages, versions,
  lines, dependency edges, implementation heads, and annotation slots per
  package. Then "which package provides the `(Validation, User)` slot" is a
  query.

## 9. Rules Summary

1. A package is one source root, one kind, one version, and one interface
   file. Unknown manifest keys are errors.
2. A dependency key is the `NAME` of `dep.NAME`. A key cannot be `std`, `pkg`,
   or `dep`.
3. Only a library can be a dependency. Only an application can declare
   executables or hold an orphan annotation.
4. An executable selects an entry module. The entry point is that module's
   public `main` or `main!`; without one, the module must be a script.
5. Test-root modules may use test dependencies. Modules in the source root
   may not.
6. A requirement is a minimum version in its compatibility line. Resolution
   is MVS.
7. At most one version per compatibility line. Distinct lines are distinct
   package identities for declarations, orphan rules, and coherence.
8. Coherence is checked over resolved interface files before any body is
   compiled. A conflict is a resolution failure.
9. The registry enforces the checked compatibility rule at publish. Any
   interface difference it cannot classify counts as breaking.
10. `hd.lock` records selections, tree hashes, and interface hashes. Path and
    git sources are allowed only in unpublished packages; git sources are
    pinned to a full commit.
11. `std` is the toolchain's. `[package] hd` states the minimum toolchain.
12. No command prompts. Every command accepts `--format json`.

## Owner Decisions

Decided 2026-09-26:

1. **Question 1: one version per compatibility line.** Two majors of one
   package may coexist (`json = "acme/json@2.1.0"`,
   `json_old = "acme/json@1.9.0"`), as distinct packages.
2. **Question 2: version ranges with a solver** (Cargo and uv style caret
   ranges, PubGrub-style resolution), not minimal version selection.
3. **Question 3: registry names are `owner/name`.**
4. **Question 4: Cargo style.** One package may have a library root and
   executables. The root-application orphan exception must be restated for
   this shape (which targets count as the root application).
5. **Question 5: `use` inside `test` blocks.** A `test` block may contain
   `use` declarations scoped to that block, and only those may name
   test-only dependencies; test builds include test dependencies, and the
   separate `tests/` root may use them anywhere.
6. **Question 6: adding an implementation or annotation for a foreign trait
   or facet is a minor change;** `hd update` reports a resulting coherence
   conflict before writing.
7. **Question 7: adding an enum variant is breaking,** for now.
8. **Question 8: each `0.MINOR` is its own compatibility line.**
9. **Question 9: git and path dependencies are not allowed in published
   packages.**
10. **Question 10: a toolchain minimum plus an optional root pin** that `hd`
    downloads; no editions yet.
11. **Question 11: published packages contain sources and the interface
    file;** Wasm components are decided with the component ABI.
12. **Question 12: no optional features or conditional compilation.**
13. **Question 13: only toolchain-defined runtime profile names,** until the
    host capability catalog is settled.
14. **Question 14: a public checksum transparency log** once a public
    registry exists.

## 10. Questions For The Owner

1. **Can two majors of one package coexist in a graph?**
   Options: (a) one version per package per graph, as in SwiftPM, uv, and
   Gradle; (b) one version per compatibility line, as in Go, MoonBit, and
   Cargo; (c) any number, as in npm.
   Recommendation: (b). It supports gradual migration, where `models`
   implements both `json@1` and `json@2` traits, and `pub use` makes the
   semver trick work. Diagnostics must print the line.
   Example: `json = "acme/json@2.1.0"` and `json_old = "acme/json@1.9.0"` in
   one manifest.

2. **Minimum-only requirements (MVS) or ranges with a solver?**
   Options: (a) MVS, minimum only; (b) caret ranges with a PubGrub solver,
   as in Cargo and uv; (c) MVS plus root-only `exclude`.
   Recommendation: (a), with `[patch]` as the only override. The registry's
   compatibility check covers MVS's trust assumption. Add (c) only if yanking
   proves insufficient.
   Example: `billing = "acme/billing@1.4.2"` means "1.4.2 or the largest
   1.x any other manifest asks for".

3. **Registry names: namespaced or flat?**
   Options: (a) `owner/name`, as in Go and MoonBit; (b) flat names, as in
   crates.io and PyPI.
   Recommendation: (a). It avoids name squatting and makes ownership
   visible. The dependency key keeps source short, so `dep.json` does not
   repeat the owner.
   Example: `json = "acme/json@2.1.0"`, used as `use dep.json.{Value}`.

4. **Entry module default, and can one package be both library and
   application?**
   Options: (a) default entry module `main` (`src/main.hd`), and a package is
   exactly one kind; (b) default entry is the root module `src/mod.hd`;
   (c) Cargo style, where one package has a library root and executables.
   Recommendation: (a). The root-application orphan exception needs an
   unambiguous role. A project that ships both uses a workspace with two
   packages.
   Example: `kind = "application"` with no `[[executable]]` runs
   `pub fn main` in `src/main.hd`.

5. **Where do test-only dependencies apply?**
   Options: (a) a separate `tests` root that sees the package's public
   surface; (b) a filename convention such as `*_test.hd`, whose modules are
   test-only; (c) no test dependencies.
   Recommendation: (a). A `use` is module-wide, so inline `test` blocks cannot
   be restricted. A separate root also tests the public API the way a
   downstream package does. (b) changes path-inferred module naming.
   Example: `tests/billing_roundtrip.hd` uses `dep.fixtures` and
   `pkg.invoice.{Invoice}`.

6. **Is adding an implementation or annotation on a foreign trait or facet
   a minor change?**
   Options: (a) minor; the conflict, if any, appears when the root raises
   the version; (b) major, because it can take a slot a root application
   filled with an orphan annotation; (c) minor, and `hd update` reports the
   possible conflict before writing.
   Recommendation: (c). A major bump for every new trait conformance would
   stall the ecosystem, and MVS confines the failure to an explicit update.
   Example: `acme/models@1.3.0` adds `annotate Validation for User`. An app
   that already had its own `annotate Validation for User` sees
   `coherence-conflict` on `hd update models`, naming both annotations.

7. **Is adding an enum variant always breaking?**
   Options: (a) yes, as drafted; (b) add a language-level marker for open
   enums that forces a wildcard arm downstream.
   Recommendation: (a) for now. (b) is a language feature and belongs in
   [Open Issues](OPEN_ISSUES.md) if wanted.
   Example: adding `Refunded` to `pub enum PaymentState` in `acme/billing`
   requires `2.0.0`.

8. **How are `0.x` versions treated?**
   Options: (a) each `0.MINOR` is its own compatibility line, as in Cargo;
   (b) `0.x` has no compatibility promise and the check is skipped, as in
   Go; (c) `0.x` versions are all one line.
   Recommendation: (a). The check still applies to patches, and coexistence
   treats `0.3` and `0.4` like majors.
   Example: `0.3.1` to `0.3.2` must keep the interface signature-equal;
   `0.3.2` to `0.4.0` may break it.

9. **Are git and path dependencies allowed in published packages?**
   Options: (a) no, as in Cargo; (b) git allowed when pinned to a commit.
   Recommendation: (a). The registry must be able to recompile and check
   every package in a published graph.
   Example: `hd publish` rejects `pdf = { git = ... }` with
   `unpublishable-dependency` and suggests publishing `pdf` first.

10. **How is the toolchain selected?**
    Options: (a) `hd` minimum only, and the user installs toolchains;
    (b) minimum plus an optional root pin that `hd` downloads, as Go's
    `toolchain` line does; (c) language editions in addition.
    Recommendation: (b), without editions for now. `std` breaking changes wait
    for a toolchain major.
    Example: `hd = "0.9.0"` in a library; `[toolchain] pin = "0.9.4"` in an
    application.

11. **What does a published package contain?**
    Options: (a) sources and interface file only; (b) also precompiled Wasm
    per package; (c) applications publish Wasm components through a separate
    channel.
    Recommendation: (a) for the registry, and decide (c) with the component
    ABI in area 3.
    Example: `acme/billing-1.4.2` contains `hd.toml`, `src/`, `tests/`, the
    interface file, `README.md`, and `LICENSE`.

12. **Optional features or conditional compilation?**
    Options: (a) none; (b) additive features as in Cargo or Swift traits.
    Recommendation: (a). One build per package version keeps one interface
    file per version, which the compatibility check and the lockfile rely on.
    Example: a JSON library that wants optional `std.time` support ships a
    second package, `acme/json_time`.

13. **Can a manifest define runtime profiles?**
    Options: (a) only toolchain-defined profile names; (b) custom profiles in
    `hd.toml` that list host capability traits.
    Recommendation: (a) until the host capability catalog in
    [Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work) is
    settled.
    Example: `profile = "console-fs"` is valid only if the toolchain defines
    `console-fs`.

14. **Should the registry run a public checksum log?**
    Options: (a) no, the lockfile hash is enough; (b) a transparency log like
    `sum.golang.org`, which `hd` checks on first download.
    Recommendation: (b) once there is a public registry. It gives every user
    the same hash for a version, even without a committed lockfile.
    Example: `hd add` fails with `checksum-mismatch` if the downloaded tree
    hash differs from the log.
