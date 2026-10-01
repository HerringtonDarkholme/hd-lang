# Packages: Survey And Manifest Draft

Status: research and design draft for [Roadmap](ROADMAP.md#order) item 3.
Nothing here is accepted language behavior. The owner decided questions
1 to 14 on 2026-09-26 ([Owner Decisions](#owner-decisions)). Dependencies
decisions DEP1-DEP19 (2026-09-29) then removed the registry: versions are
git tags, resolution is minimal version selection, and `hd.sum` gives
integrity. They are applied in
[Package Manifest](../spec/10-modules.md#package-manifest), which is
authoritative. So the registry names, caret ranges, solver, lockfile,
and distribution that this draft first proposed are superseded, and
their text is in git history. What stays open here is the final
`hd.toml` schema ([`module.tooling.package-schema`](../spec/10-modules.md#r-module.tooling.package-schema)),
the checked compatibility rule behind `hd api diff`, and the agent-first
CLI; the tooling plan is in
[Package Tooling](archive/RUNTIME_AND_LIBRARY.md#package-tooling).

Inputs:

- [Package Manifest](../spec/10-modules.md#package-manifest),
  [Use Roots](../spec/10-modules.md#use-roots),
  [Name Resolution Across Packages](../spec/10-modules.md#name-resolution-across-packages)
  (interface files, link-time coherence), and
  [Executable Entry Point](../spec/10-modules.md#executable-entry-point).
- The orphan and overlap rules in [Traits](../spec/09-traits.md). The
  annotation coherence rules were removed with the facet protocol
  ([Typed Derivation decision 10](../spec/14-annotations.md#typed-derivation)), and
  the root-application orphan exception is dropped (decision 4).
- The package bullet in
  [Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work).

Current state. The prototype in `src/` does not read `hd.toml`. The
conformance runner simulates a package graph instead: it passes
`--package-role library|root-application` and one `--dependency NAME=DIR` per
directory under [`spec/conformance/packages/`](../spec/conformance/packages),
and each such directory is a source root whose `mod.hd` is the package root
module ([Package Roles](../spec/conformance/README.md#package-roles)). The
draft below keeps that model: a manifest dependency key is the `NAME` in
`dep.NAME`. Under decision 4 one package may have both a library and
executables, so the role is no longer one package kind; which targets take
the root-application role is open.

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

1. **MVS was this draft's first choice; the owner chose ranges with a
   solver.** MVS is deterministic and explainable in one sentence ("the
   largest minimum anyone asked for"), and its trust assumption is what hd's
   interface files let the registry check. Decision 2 instead adopts caret
   ranges with PubGrub-style resolution, as Cargo and uv do.
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
8. **Separate dependency groups for tests.** hd's use declarations are
   module-wide, so test-only dependencies need a boundary, like MoonBit's
   `test-import` and Go's `_test.go` files. The testing redesign gives
   three: a file's [`tests:` block](../spec/02-grammar.md#test-blocks), a
   `_test.hd` test module, and the separate `tests/` root
   ([Test Modules](../spec/10-modules.md#test-modules)).

## 2. Draft: `hd.toml`

The examples and schema below predate DEP1. Their registry forms
(`owner/name@X.Y.Z`, `id`, `registry`), `git` sources, the `version`
field, and `hd.lock` are superseded by
[Package Manifest](../spec/10-modules.md#package-manifest); the rest is
the open schema draft.

### 2.1 Design Goals

- One package is one source root, one version, and one interface file. Like
  a Cargo package, it may have a library, executables, or both (decision 4).
- The manifest is plain TOML with a closed schema. Every key has one meaning.
  Unknown keys are errors.
- Tools edit the manifest; humans read it. `hd` rewrites it in a canonical
  key order, so an agent's edit produces a minimal diff.
- Nothing in the manifest duplicates what source already states. Provider
  bindings are derived from entry points
  ([Runtime and Library Design](archive/RUNTIME_AND_LIBRARY.md)), and public API is
  derived from `pub` declarations.

### 2.2 Full Example: Application

```toml
[package]
name = "invoice_cli"
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

[toolchain]
pin = "0.9.4"
```

This package has executables and no library: its source root has no
`mod.hd`. `[toolchain] pin` is allowed only in a root manifest
(decision 10).

### 2.3 Full Example: Library

```toml
[package]
name = "billing"
id = "acme/billing"
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

This package has a library, because `src/mod.hd` exists, and no executables.
Adding an `[[executable]]` table would give it both, as a Cargo package with
`src/lib.rs` and `src/main.rs` has.

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
| `id` | `owner/name` | Registry identity (decision 3). Required to publish. `name` must equal its last component. |
| `version` | `MAJOR.MINOR.PATCH[-PRE]` | See [Versions](#3-versions). Build metadata (`+...`) is rejected. |
| `hd` | version | Minimum toolchain version, which is also the minimum `std` version. |
| `description`, `license`, `repository`, `readme` | strings | Metadata. `license` is an SPDX expression. Required to publish. |

`[source]`, optional:

| Key | Default | Rule |
| --- | --- | --- |
| `root` | `"src"` | Source root for path-inferred modules ([Path-Inferred Modules](../spec/10-modules.md#path-inferred-modules)). `root/mod.hd`, when present, is the package root module and public index, and its presence gives the package a library. |
| `tests` | `"tests"` if the directory exists | Test source root of integration test modules ([Test Modules](../spec/10-modules.md#test-modules)). Only a test build compiles them. They may use dependencies, test dependencies, and one another, and they see only the package's public surface, built without its test code, as a dependent would. |

The two roots must not overlap. Within `root`, a test dependency may be
named only by test code: a `use` inside a file's
[`tests:` block](../spec/02-grammar.md#test-blocks), or any `use` of a
`_test.hd` test module. Other uses in `root` may name only `[dependencies]`,
because those modules are also part of the normal build. Test builds include
the test dependencies.

A test dependency that itself depends on this package may be used only from
`tests/`. From a `tests:` block or a test module it is an error, since it
would bring in a second copy of the package ([Testing T24](../spec/10-modules.md#test-modules)).

A package must have a library, at least one executable, or both. Only a
package with a library can be a dependency; a dependent sees its library and
never builds its executables. No target has an orphan exception
(decision 4).

`[[executable]]`, zero or more:

| Key | Default | Rule |
| --- | --- | --- |
| `name` | package `name` | Output artifact name. Unique within the package. |
| `module` | `"main"` | Entry module, as a module path relative to the source root. |
| `profile` | `"console"` | Runtime profile ([Wasm Boundary](../spec/10-modules.md#wasm-boundary)). Only toolchain-defined profile names are allowed (decision 13). |

If a package has no library and declares no `[[executable]]`, it has one
implicit entry with all defaults, so `src/main.hd` is the entry module. See
[Entry Points](#26-entry-points) below for the selection rule.

`[dependencies]` and `[test-dependencies]`: each key is an identifier and
becomes the `NAME` in `dep.NAME`. Each value is one of:

| Form | Meaning |
| --- | --- |
| `"owner/name@X.Y.Z"` | Registry package, caret range: at least `X.Y.Z`, within its compatibility line. |
| `{ id = "owner/name", version = "X.Y.Z", registry = "URL" }` | Registry package from a non-default registry. |
| `{ path = "DIR" }` | Local package. Not allowed in a published package (decision 9). |
| `{ git = "URL", rev = "SHA" }` | Git package at one full commit hash. Branches and tags are rejected. Not allowed in a published package (decision 9). |

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

`[toolchain]`: root only. `pin` names an exact toolchain version, which `hd`
downloads when it is missing (decision 10). There are no editions.

There is no table for optional features or conditional compilation
(decision 12).

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
6. A package without executables cannot be run. `hd run` on it fails with
   a diagnostic that says it has only a library.

The manifest never names the function. That keeps one source of truth: an
agent that renames `main` sees a checker error in source, not a stale
manifest.

## 3. Versions

### 3.1 Syntax

A version is `MAJOR.MINOR.PATCH` with an optional `-PRE` pre-release suffix
using SemVer 2.0.0 ordering. All three numeric parts are required in the
manifest, the lockfile, and CLI output. Build metadata is rejected.

A dependency requirement names one tag. The decided requirement forms
are in [Dependency Requirements](../spec/10-modules.md#dependency-requirements);
the caret ranges of decision 2 are superseded.

### 3.2 Compatibility Lines

The compatibility line of a version is its major number when the major is at
least 1, and `0.MINOR` when the major is 0 (decision 8). Two versions in one line must be
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
| Add an enum variant | breaking | `match` is exhaustive and hd has no marker for open enums (decision 7). |
| Add a trait method with a default | compatible | Subject to area 2's default-method conflict rules. |
| Add a trait method without a default | breaking | Existing implementations fail. |
| Add a `mut self` method, even with a default, to a requirement trait that had none, directly or through a supertrait | breaking | `$.use` then yields `mut K`, so installing a readonly provider becomes `mutable-upgrade` ([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)). |
| Add an inherent method | compatible, pending area 2 | Safe only if inherent lookup cannot change which method an existing call selects. |
| Add an implementation whose trait and target the package both own | compatible | No other package can hold that slot. |
| Add a generic (blanket) implementation | breaking | It can overlap an implementation in a downstream package. |
| Add an implementation or annotation whose trait or facet another package owns | compatible (decision 6) | With no orphan exception, only the owning packages can fill such a slot, so no dependent can already hold it. `hd update` still reports any coherence conflict before writing. |

The registry runs `hd api diff` against the highest published version of the
same line below the new one, and refuses a publish whose declared version is
smaller than the required bump. A larger bump is always allowed. The first
release of a line is unchecked.

For `std`, the same rule applies across toolchain releases. One extra rule
applies: adding a prelude name is breaking, because any downstream
declaration of that name becomes a `prelude-name-shadow` error.

## 4. Resolution

### 4.1 Algorithm

Superseded by DEP1: resolution is minimal version selection
([Version Selection](../spec/10-modules.md#version-selection)).

### 4.2 Coherence Across Packages

The link-time check runs over the complete set of resolved interface files
([Name Resolution Across Packages](../spec/10-modules.md#name-resolution-across-packages)):

- At most one implementation per instantiated trait and target, with overlap
  checked across packages ([Traits](../spec/09-traits.md)).
- No orphan implementations or derivations in any package; a foreign type
  is derived through a local mirror type or newtype (decision 4).

Because interface files contain every implementation head, `hd resolve`
runs this check before compiling any function body. A registry that serves
interface files lets `hd resolve` report a conflict before sources are
downloaded.

A locked dependency version changes only when a command re-resolves, so
any coherence failure appears on the `hd add` or `hd update` that selected
the new version, and `hd update` reports it before writing (decision 6). It
never appears on a later unrelated build.

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

Owner decision 1 chose this rule.

## 5. Lockfile

Superseded by DEP1 and DEP4: there is no lockfile, and `hd.sum` is the only
integrity source ([Package Manifest](../spec/10-modules.md#package-manifest)).
Its line format is tooling work in
[Package Tooling](archive/RUNTIME_AND_LIBRARY.md#package-tooling).

## 6. Standard Library

- `std` ships inside the toolchain and is never downloaded or listed as a
  dependency. Its version is the toolchain version.
- `[package] hd` is a minimum. The selected toolchain must be at least the
  largest `hd` value in the resolved graph, as with Go's `go` line.
- The root may pin a toolchain with `[toolchain] pin`, which `hd` downloads
  (decision 10). There are no language editions yet.
- `std` follows the checked compatibility rule across toolchain releases in
  one line, with the prelude rule from [section 3.3](#33-the-checked-compatibility-rule).
  A breaking `std` change needs a new toolchain major.
- The toolchain version recorded in `hd.lock` is informative. `--locked`
  fails only if the selected toolchain is below the graph's minimum.

## 7. Distribution

Superseded by DEP1: there is no registry and no publish step, and a
version is the tagged tree.

## 8. Agent-First CLI

Every package operation is a non-interactive command. No command prompts.
Anything that would need confirmation fails with a diagnostic that names the
flag to pass.

| Command | Effect |
| --- | --- |
| `hd new [--library] [--executable] NAME` | Create a package skeleton with a library, an executable, or both. |
| `hd add NAME ID@VERSION [--test]` | Add or raise a dependency, re-resolve, and update `hd.lock`. Without a version, use the newest non-yanked release. |
| `hd remove NAME` | Remove a dependency. |
| `hd update [NAME]` | Re-resolve to the newest versions the ranges allow and rewrite `hd.lock`, reporting any coherence conflict before writing. `--line` also moves the manifest requirement to a new line. |
| `hd resolve [--explain ID]` | Resolve and check coherence. `--explain` lists each requirer of the package and its range, and on failure the solver's derivation. |
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
- Package metadata is exposed to the program database, which is
  [on hold](ROADMAP.md#on-hold): packages, versions,
  lines, dependency edges, implementation heads, and annotation slots per
  package. Then "which package provides the `(Validation, User)` slot" is a
  query.

## Owner Decisions

Decided 2026-09-26. The options weighed for each are in git history.

| # | Decision | Status |
| --- | --- | --- |
| 1 | One version per compatibility line; two majors coexist as distinct packages | Applied in [Package Manifest](../spec/10-modules.md#package-manifest) |
| 2 | Caret ranges with a solver | Superseded by DEP1: minimal version selection |
| 3 | Registry names are `owner/name` | Superseded by DEP1: no registry; a dependency is named by its host path |
| 4 | Cargo style: a library, executables, or both; no package holds an orphan | Applied |
| 5 | `use` inside `test` blocks | Superseded by the testing redesign ([Test Modules](../spec/10-modules.md#test-modules)) |
| 6 | Adding an implementation for a foreign trait is a minor change | For `hd api diff`, which DEP7 schedules later ([3.3](#33-the-checked-compatibility-rule)) |
| 7 | Adding an enum variant is breaking, for now | For `hd api diff`, as 6 |
| 8 | Each `0.MINOR` is its own compatibility line | Applied |
| 9 | No git or path dependencies in released versions | Kept by DEP15 ([`module.version.no-path-release`](../spec/10-modules.md#r-module.version.no-path-release)) |
| 10 | A toolchain minimum plus an optional root pin; no editions | Applied ([Toolchain Version](../spec/10-modules.md#toolchain-version)) |
| 11 | Published packages hold sources and the interface file | Superseded by DEP1: no publish step |
| 12 | No optional features or conditional compilation | Applied |
| 13 | Only toolchain-defined runtime profile names | Applied ([Runtime Profiles](../spec/10-modules.md#runtime-profiles)) |
| 14 | A public checksum log once a registry exists | Moot under DEP1 and DEP4 |

## 10. Questions For The Owner

All fourteen questions were decided on 2026-09-26; see
[Owner Decisions](#owner-decisions).
