# Releasing a package

Status: design proposal, 2026-10-23; domain: CLI/tooling; not accepted.
This revises [#28](https://github.com/HerringtonDarkholme/hd-lang/issues/28)
under its [owner review](https://github.com/HerringtonDarkholme/hd-lang/issues/28#issuecomment-6372612840).
It proposes tooling over [package versions](../../spec/lang/10-modules.md#versions)
and [public interfaces](../../spec/lang/10-modules.md#public-signatures).
No command, compatibility policy, or new specification rule is accepted here.

## Contents

- [Problem, users, use cases and today](#problem-users-use-cases-and-today)
- [Prior art, tool by tool](#prior-art-tool-by-tool)
- [What is compatible](#what-is-compatible)
- [Worked examples](#worked-examples)
- [Options](#options)
- [Where the API comes from](#where-the-api-comes-from)
- [Architecture and implementation](#architecture-and-implementation)
- [Command and tags](#command-and-tags)
- [Extension points](#extension-points)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Phasing](#phasing)
- [Recommendation](#recommendation)
- [Approval and validation](#approval-and-validation)

## Problem, users, use cases and today

Package maintainers need a suggested version before tagging a release.
Reviewers need the reason for that suggestion, including breaks hidden by source-level defaults or inferred types.
CI needs a deterministic report and an explicit version check.
The following workflows are proposals, not commands available today.

| User and goal | Commands and report | Decision and completion |
| --- | --- | --- |
| Author adds a standalone helper | bare `hd release` reports `minor`, with the added public path | Review behavior, then `hd release 1.5.0` checks and creates a local tag. |
| Author adds a requirement to an existing function | bare `hd release` shows the old/new rows and `major` | Update clients, then `hd release 2.0.0` checks and tags it. |
| CI reviews a proposed release | `hd release 1.5.0 --dry-run --format json` returns findings and version adequacy | Reject an insufficient bump; a bare `hd release` alone remains informational. |
| Workspace author releases one member | `hd release -p lint --since 1.4.0`, then `hd release -p lint 1.5.0` | Inspect only that package's comparison and create `lint/v1.5.0`. |
| Author makes the first release | bare `hd release` reports `first-release`, with no compatibility verdict | Run `hd release 0.1.0` (or another exact version). The report shows no minimum bump. |

Today, versions are [Git tags](../../spec/lang/10-modules.md#r-module.version.tag),
with [directory prefixes](../../spec/lang/10-modules.md#r-module.version.tag-prefix).
A manifest [does not state its own version](../../spec/lang/10-modules.md#r-module.version.tag-only).
[Release compatibility tooling](../../spec/cli/command-line.md#r-cli.tooling.package-later) remains outside the accepted CLI specification.

At `a93482cb9`, neither proposed command appears in `hd help`.
The earlier `hd doc --format json pkg.answer` probe produced rendered Markdown plus summary JSON, not typed API records.
Source inspection confirms that documentation rendering omits some implementation kinds.
The revision therefore uses checked interfaces and an explicit release projection.

## Prior art, tool by tool

### Elm: diff reads, bump writes

Elm builds exposed modules with typed documentation retained, then compares structured API data against a published version.
`elm diff` reports changed modules and declarations with a SemVer magnitude.
`elm bump` uses the comparison to choose a permitted version, asks for confirmation, and updates `elm.json`.
It neither creates hd-style Git versions nor proves behavioral compatibility.

The useful precedent is the read/write split and structured types, rather than a diff of rendered pages.
Its package registry supplies baselines; hd would use exact local Git objects instead.
Elm's source separates [diff construction/reporting](https://github.com/elm/compiler/blob/master/terminal/src/Diff.hs)
from [version mutation](https://github.com/elm/compiler/blob/master/terminal/src/Bump.hs).
Its type-driven enforcement cannot substitute for hd's row, impl, and default rules.

### cargo-semver-checks: typed API queries

`cargo semver-checks --baseline-rev TAG` compares a Rust crate with a Git baseline.
It consumes rustdoc JSON and expresses compatibility checks as queries, rather than re-parsing rendered documentation.
Baseline versions, source roots, selected features, and targets affect coverage.
Its findings explain individual violations instead of returning an unexplained version number.

The JSON format tracks Rust compiler changes, so toolchain compatibility is a maintenance cost.
A selected feature/target configuration cannot establish every other configuration's compatibility.
This supports hd's compiler-owned projection and explicit provenance; it does not establish body-level behavior.
See the project's [data source, baseline options, configuration, and exit statuses](https://github.com/obi1kenobi/cargo-semver-checks).

### cargo-release: orchestration with explicit execution

`cargo release` previews by default; `--execute` performs its configured release steps.
Version changes, commits, tags, publication, pushes, and hooks are separable steps.
Workspace selection and configurable tag naming cover larger release workflows.
The tool orchestrates a release; a type-based compatibility checker remains a separate concern.

Its broad configuration and external effects are unnecessary for hd's initial feature.
The proposed writer creates one checked local tag, with no publication, hooks, commits, or pushes.
Unlike its preview-first surface, hd uses a separately named reader and an exact-version writer.
See [cargo-release's reference](https://github.com/crate-ci/cargo-release/blob/main/docs/reference.md).

### Go: gorelease and apidiff

`gorelease [-base=VERSION] [-version=VERSION]` compares the main module's public API and dependencies.
Without `-version`, it suggests a minimum version; with it, it checks the proposed version.
The experimental tool reports problems rather than performing the release.
Its Go-specific module paths and permissive major-zero policy should not replace hd's compatibility lines; see [gorelease](https://pkg.go.dev/golang.org/x/exp/cmd/gorelease).

The underlying `apidiff.Changes(old, new)` consumes checked `go/types.Package` values.
It tracks corresponding types across versions instead of comparing printed names alone.
Exported function signatures matter even when direct calls would still work, because clients can store function values.
Its compatibility model excludes some unusual clients; see [apidiff's API and compatibility discussion](https://pkg.go.dev/golang.org/x/exp/apidiff).

### npm version: explicit mutation without API inference

`npm version patch` or an exact version updates package version files and normally creates a Git commit and tag.
Configured `preversion`, `version`, and `postversion` scripts can execute during the workflow.
A clean-tree check protects the mutation unless explicitly overridden.
This command writes version state; it does not infer compatibility from typed public interfaces.

hd has no manifest version to update and needs no release-script mechanism for this feature.
The relevant precedent is that a write-oriented command visibly mutates state.
Copying npm's scripts or force option would add effects unrelated to comparing APIs.
See [npm version's documented steps](https://docs.npmjs.com/cli/v11/commands/npm-version/).

## What is compatible

The comparison asks whether existing source clients and downstream implementers retain their typed interface.
It does not promise binary compatibility or identical behavior.
[SemVer](https://semver.org/spec/v2.0.0.html) requires a declared public API; these proposed rules define the tool's checked portion of that API.
The author still owns documented contracts, including behavior that types cannot express.

A finding has a magnitude and evidence: `definite` names a known invalid old use; `potential` identifies a conservative source risk.
Both breaking kinds require the same bump.
`unassessed` is separate: missing facts, failed checks, or unsupported extraction prevent a verdict and refuse tagging.
It never silently becomes `patch`.

| Aggregate | Exact proposed meaning |
| --- | --- |
| `major` | At least one definite or potential break below, even alongside additions. |
| `minor` | A supported compatible addition below, with no breaking finding. |
| `patch` | No checked API change after normalization; behavior remains author-reviewed. |
| `unassessed` | Either snapshot or comparison is incomplete; no suggested release is certified. |
| `first-release` | No earlier matching tag exists; comparison is inapplicable, not an extraction success against an empty API. |

### Functions and their rows

[Named argument labels](../../spec/lang/07-functions.md#r-fn.arg.labels) are parameter names.
[Function references](../../spec/lang/07-functions.md#r-fn.ref.value) discard names and defaults, but retain arity, types, rows, and suspension.
A direct call is therefore only one supported client form.
Compare the complete declaration and its function-value type.

| Edit to an existing exported function or method | Finding |
| --- | --- |
| Remove it; change parameter count/order/label, parameter or result type, receiver permission, variadic shape, or suspension | `major`; definite for the affected call, reference, or annotation. Type widening is not automatically safe inside invariant containers. |
| Add a defaulted value parameter | `major`; direct calls may survive, stored function values change arity. |
| Remove an existing parameter default | `major`; omitted-argument calls fail. |
| Add a default to an existing parameter without changing its type or position | `minor`; new call form, same existing references. |
| Change an ordinary value-default expression, keeping its type and presence | `patch` for typed API, with a behavioral-change note. |
| Change a normalized row in either direction | `major`; added requirements break direct callers, removed requirements can break inferred nested function types. |
| Reorder row keys or expand a row alias to the same keys | No finding; normalize aliases, bindings, key order, and duplicates first. |
| Add a fresh module-level function or public alias path to an existing declaration | `minor`, provided no old path disappears or changes identity. |
| Add a public inherent method to an existing type | Potential `major`; inherent lookup can replace an old trait call. |

[Row subsumption](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume)
still permits a function value at a broader expected row.
That adaptation does not recursively rewrite a row stored inside an inferred generic container.
[Declared variance](../../spec/lang/04-type-system.md#r-types.variance.function) keeps function rows invariant there.
The row-shrinking example below is the counterexample to this note's earlier compatibility claim.

### Types, fields and variants

Nominal identity follows old public paths and their equality relationships, not a run-local `DefId`.
[Re-exports preserve identity](../../spec/lang/10-modules.md#r-module.pub-use.identity).
A transparent alias is compared through its resolved target; a newtype remains distinct.
[Member resolution](../../spec/lang/03-names-and-scopes.md#member-resolution) also requires tracking promoted members and inherent lookup.

| Edit | Finding |
| --- | --- |
| Remove a public type/path, split one formerly identical type into two, merge distinct exposed identities, or change struct/enum/newtype kind | `major`; old names, assignments, or patterns can fail. |
| Change a newtype base or resolved alias target | `major`, unless normalized target types remain equal. |
| Add a required field to an externally constructible struct | `major`; an old literal omits it. |
| Add a public defaulted struct field to an existing type | Potential `major`; old literals and subset patterns survive, but a downstream embedding may acquire a promoted-name collision. |
| Add a private field to a formerly externally constructible struct | `major`, even with a default; all fields must be public for external literals. |
| Add a hidden storage field to an already opaque struct | `patch` if no public lookup or effective impl contract changes; note possible behavior changes. |
| Remove/rename/change an old public field, or reduce its visibility | `major`; field access, literals, and patterns are clients. |
| Add a public field to an opaque struct, or expose an existing field | Potential `major`; downstream promotion can change even when construction was already opaque. |
| Add/remove an embedded field, or change an old promoted path's selection | Potential `major`; compare resolved promotion paths, not just direct fields. |
| Add/remove/rename an enum variant | `major`; construction or exhaustive matching breaks. |
| Change variant payload count/order/labels/types or generic shape | `major`; constructor calls, constructor values, and payload patterns differ. Defaults do not erase those clients. |
| Add shared enum data | `minor` if existing public fields/constructors/impls remain unchanged; otherwise apply those breaks. |
| Reorder independent declarations or named struct fields | No typed finding; default evaluation, derivation order, and serialization still receive behavior notes. |

Struct literals can [omit defaulted fields](../../spec/lang/08-data-and-enums.md#r-data.literal.defaulted).
Struct patterns may [mention a subset](../../spec/lang/08-data-and-enums.md#r-data.pattern.subset), which makes a defaulted field addition different from a variant addition.
[External literal visibility](../../spec/lang/08-data-and-enums.md#r-data.vis.literal)
and [closed enums](../../spec/lang/08-data-and-enums.md#r-data.enum.closed) supply the other boundaries.
Private field types remain available to effective-impl analysis without becoming exported members.

### Traits, impls and generics

[Trait defaults and exact implementation signatures](../../spec/lang/09-traits.md#r-trait.impl.override)
make downstream implementers clients of the trait declaration.
[Bound-only inference](../../spec/lang/04-type-system.md#r-types.generic.infer.bound)
can depend on there being exactly one instantiation.
Adding an impl is therefore not automatically a minor change.

| Edit | Finding |
| --- | --- |
| Remove a trait/member/associated type, change a member's complete contract, or add a required member | `major` for callers or downstream implementations. |
| Add a defaulted trait member or associated type | Potential `major`; new lookup candidates or downstream member conflicts need no missing-method error to break a client. |
| Add/remove/change supertraits or associated-type bounds/defaults | Potential `major`; obligations, associated projections, or lookup can change. |
| Remove a method's default body without removing the method | `major`; implementations relying on it become incomplete. |
| Add a default body to an existing required method, keeping its contract | `minor`; old implementations remain valid. Changing only an existing body is `patch` plus behavior review. |
| Remove an externally applicable impl, change its trait arguments/target/associated bindings, or edit its bounds | `major`; exact applicability and inferred projections must remain available. Bounds relaxed as well as tightened receive potential findings. |
| Add an impl whose target is an unchanged old type, a primitive, a tuple, or a generic pattern | Potential `major`; inference, method lookup, or overlap with downstream impls can change. |
| Add an impl only for a fresh nominal target, or only for a fresh trait | `minor` only when its target/trait pattern cannot match an old observable pair and it changes no old member lookup. Unproved separation is potential `major`. |
| Add a fresh standalone trait/type with no effects on old impls or paths | `minor`. |
| Change generic count/order/kind/variance/bounds/default type or row | Potential `major`; compare uses, annotations, downstream impls, and inference. A newly defaulted generic slot is not the ordinary value-default exception. |
| Rename a generic binder while keeping its ordinal, kind, and uses | No finding; normalize binders to owner-relative ordinals. Ordinary value-parameter labels remain significant. |

The first implementation proves separation only for a fresh nominal head or a fresh trait identity.
It requires every relevant old pattern to be structurally disjoint and every old resolved member selection unchanged.
An old blanket pattern or unresolved projection defeats that proof and produces a potential break.
It must not enumerate only today's concrete instantiations and claim future generic clients are covered.

[Wildcard imports are forbidden](../../spec/lang/10-modules.md#r-module.use.no-wildcard),
and a new trait is not [injected into every scope](../../spec/lang/09-traits.md#r-trait.avail.no-injection).
Those rules justify some fresh-name additions.
They do not make a new impl of an already available trait harmless.
For an unchanged existing contract, a changed implementation body alone still receives behavioral review.

### Visibility, defaults, and @derive changes

The public surface includes every importable public declaration under the package's library modules, not merely root-facade exports.
It also includes re-export paths, constructor access, public methods, and applicable impls.
[Public signature checks](../../spec/lang/10-modules.md#public-signatures) reject private types leaking through that surface.
A private implementation helper is not itself a versioned public name.

| Edit | Finding |
| --- | --- |
| Make an old public name private, or remove a facade while retaining its declaration elsewhere | `major`; the old path remains an obligation. |
| Expose a private item without changing old lookup | `minor`; body-local and test-only names remain excluded. |
| Edit value defaults or shared-data initializer values, retaining typed contracts | `patch` plus behavior note; changed generic defaults instead follow the generic-contract rule. |
| Remove `@derive(Eq)` or another externally applicable generated impl | Same `major` finding as removing a written impl. |
| Add a derive | Compare generated impls using the impl-addition rules, including bounds and associated types. |
| Replace a derive with an equivalent written impl, or reorder decorators without changing effective contracts | No typed change; behavior review remains necessary. |
| Change a publicly usable derivation template, walker obligations, or compiler-recognized API metadata | Potential `major`; missing extractor support is `unassessed`, not ignored. |

[Derivation generates ordinary impls](../../spec/lang/09-traits.md#r-trait.derive.generate),
and [field-dependent bounds](../../spec/lang/09-traits.md#r-trait.derive.bounds) can change after a data edit.
Compare effective checked impls after expansion; comparing only decorator spelling misses both effects.
Preserve public derivation-template metadata as a normalized syntax digest when it influences downstream generated code.
A digest difference is a conservative finding, not a claim to understand arbitrary template behavior.

### What is undecidable from signatures alone

Equal signatures cannot prove equal return values, panic conditions, I/O order, cancellation behavior, or termination.
They cannot establish serialization formats, equality/hash laws, performance guarantees, or documented behavior of callbacks.
An empty requirement row is not a proof of purity, and a default can compute different values without changing its type.
Such changes can require a larger version than the tool suggests.

[References and copies](../../spec/lang/13-values-and-references.md),
[control flow and cleanup](../../spec/lang/06-control-flow.md), and
[suspension](../../spec/lang/11-requirements-and-suspension.md) retain their existing semantics.
The extractor records their typed contract where exposed, including `&`, `&mut`, bang functions, and rows.
It does not execute bodies, callbacks, initializers, `defer`, or cancellation paths.
No changes to ownership passes, the inliner, host grants, grammar, or std APIs are proposed.

Body/default syntax digests may report changed source for review; equal digests still do not prove equal behavior through changed dependencies.
The report always states `behavior_assessed: false`.
An increased minimum toolchain produces a separate potential `major` finding for clients using the old minimum.
Initial implementation checks both trees with one compiler; minimum-toolchain certification remains explicitly unassessed.

## Worked examples

Each example uses two versions of library package `api` and an unchanged package `client`.
The client's manifest selects `api = { path = "../api" }` for the probe only.
A real tagged manifest cannot retain an [unversioned path dependency](../../spec/lang/10-modules.md#r-module.version.no-bare-path-release).
The examples test language behavior, not an implemented release classifier.

### Adding a row breaks a caller

Old `api/src/lib.hd`:

```text
pub fn answer() -> i32: +42
```

New `api/src/lib.hd`:

```text
use std.time.Clock

pub fn answer() -> i32 $ Clock: +42
```

Unchanged `client/src/main.hd`:

```text
use dep.api.answer

pub fn main() -> void:
    if answer() != +42:
        panic("wrong answer")
```

Both libraries check; the new client reports `missing-requirement`.
This is a definite `major` finding, even though the function body is unchanged.

### A defaulted parameter still changes a function value

Old `api/src/lib.hd`:

```text
pub fn shift(value: i32) -> i32: value
```

New `api/src/lib.hd`:

```text
pub fn shift(value: i32, extra: i32 = +0) -> i32: value + extra
```

Unchanged `client/src/main.hd`:

```text
use dep.api.shift

pub fn main() -> void:
    let callback: fn(i32) -> i32 = shift
    if callback(+3) != +3:
        panic("wrong callback")
```

The new client reports `type-mismatch`: it expects `fn(i32) -> i32` and finds `fn(i32, i32) -> i32`.
Preserving `shift(+3)` does not preserve this program.

### Adding a default without changing arity is a minor addition

Old `api/src/lib.hd`:

```text
pub fn shift(value: i32, extra: i32) -> i32: value + extra
```

New `api/src/lib.hd`:

```text
pub fn shift(value: i32, extra: i32 = +0) -> i32: value + extra
```

Unchanged `client/src/main.hd`:

```text
use dep.api.shift

pub fn main() -> void:
    let callback: fn(i32, i32) -> i32 = shift
    if callback(+3, +1) != +4:
        panic("wrong callback")
```

Both clients check and run on both engines: the stored function type is unchanged.
The new declaration additionally accepts a call omitting `extra`, so this is a `minor` addition rather than a `patch`.

### A defaulted struct field can preserve old literals and patterns

Old `api/src/lib.hd`:

```text
pub struct Point:
    pub x: i32
```

New `api/src/lib.hd`:

```text
pub struct Point:
    pub x: i32
    pub y: i32 = +0
```

Unchanged `client/src/main.hd`:

```text
use dep.api.Point

fn x_of(point: Point) -> i32:
    match point:
        Point { x } => x

pub fn main() -> void:
    point := Point { x: +1 }
    if x_of(point) != +1:
        panic("wrong field")
```

Both clients check and run on Node and wasmtime.
This client proves that subset patterns and omitted defaults survive the change.
It does not justify `minor` for every client: the following embedding is a counterexample.

### The same defaulted field can break downstream embedding

Old `api/src/lib.hd`:

```text
pub struct Point:
    pub x: i32
```

New `api/src/lib.hd`:

```text
pub struct Point:
    pub x: i32
    pub y: i32 = +0
```

Unchanged `client/src/main.hd`:

```text
use dep.api.Point

struct Extra:
    pub y: i32

struct Combined:
    Point
    Extra

pub fn main() -> void:
    value := Combined { Point: Point { x: +1 }, Extra: Extra { y: +2 } }
    if value.x + value.y != +3:
        panic("wrong promotion")
```

The new client reports `ambiguous-promoted-member` at its `Combined` declaration.
The library's own lookup remained valid; the collision exists only in downstream code.
The proposed policy therefore treats a new public struct field as potential `major`, including a defaulted one.
This consciously favors conservative detection over Go apidiff's documented embedding exception.

### A variant addition invalidates an exhaustive match

Old `api/src/lib.hd`:

```text
pub enum Mode:
    Off
    On
```

New `api/src/lib.hd`:

```text
pub enum Mode:
    Off
    On
    Auto
```

Unchanged `client/src/main.hd`:

```text
use dep.api.Mode

fn code(mode: Mode) -> i32:
    match mode:
        .Off => +0
        .On => +1

pub fn main() -> void:
    if code(.Off) != +0:
        panic("wrong mode")
```

The new client reports `nonexhaustive-match`, with `no arm matches Auto`.
An added variant is a definite `major` finding.

### Removing a derive removes an operator contract

Old `api/src/lib.hd`:

```text
@derive(Eq)
pub struct Point:
    pub x: i32
```

New `api/src/lib.hd`:

```text
pub struct Point:
    pub x: i32
```

Unchanged `client/src/main.hd`:

```text
use dep.api.Point

pub fn main() -> void:
    if Point { x: +1 } != Point { x: +1 }:
        panic("unequal")
```

The new client reports `type-mismatch`: ``Point does not implement `Eq` for this operand``.
A source-only comparison of function signatures would miss this definite break.

### Shrinking a row can break a nested inferred type

Old `api/src/lib.hd`:

```text
use std.time.Clock

pub fn answer() -> i32 $ Clock: +42
```

New `api/src/lib.hd`:

```text
pub fn answer() -> i32: +42
```

Unchanged `client/src/main.hd`:

```text
use dep.api.answer
use std.time.Clock

struct Holder[F]:
    call: F

fn take(holder: Holder[fn() -> i32 $ Clock]) -> i32 $ Clock:
    (holder.call)()

pub fn main() -> void $ Clock:
    saved := Holder { call: answer }
    if take(saved) != +42:
        panic("wrong saved call")
```

The new client's inferred `saved` has type `Holder[fn() -> i32]`.
It cannot become `Holder[fn() -> i32 $ Clock]`; the checker reports `type-mismatch` and explains row invariance inside types.
An explicit expected function type before constructing `Holder` would adapt the value, but requiring that client edit is a break.

### An extra impl can make inference ambiguous

Old `api/src/lib.hd`:

```text
pub trait Source[T]:
    fn read(&self) -> T

pub struct Feed: pass

impl Source[i32] for Feed:
    fn read(&self) -> i32: +7

pub fn read_source[U, S < Source[U]](source: S) -> U:
    source.read()
```

New `api/src/lib.hd`:

```text
pub trait Source[T]:
    fn read(&self) -> T

pub struct Feed: pass

impl Source[i32] for Feed:
    fn read(&self) -> i32: +7

impl Source[string] for Feed:
    fn read(&self) -> string: "seven"

pub fn read_source[U, S < Source[U]](source: S) -> U:
    source.read()
```

Unchanged `client/src/main.hd`:

```text
use dep.api.{Feed, read_source}

pub fn main() -> void $ Console:
    value := read_source(Feed {})
    println(value)
```

The old client prints `7` on both engines.
The new client reports `ambiguous-type`, followed by a `cannot-infer-type` consequence at `println`.
Both libraries themselves check, so checking only the package being released would miss the break.

### Binder spelling is not a public label

Old `api/src/lib.hd`:

```text
pub fn identity[T](value: T) -> T: value
```

New `api/src/lib.hd`:

```text
pub fn identity[U](value: U) -> U: value
```

Unchanged `client/src/main.hd`:

```text
use dep.api.identity

pub fn main() -> void:
    if identity::[i32](+4) != +4:
        panic("wrong identity")
```

Both clients check and run on both engines.
The explicit type-argument list uses ordinal slots, so this isolated binder rename is `patch`.
Renaming `value` instead would change a named-call label and require `major`.

## Options

The same five workflows above apply to every option.
The smallest useful tool is an exact-version writer that checks the package, while the author chooses compatibility manually.
It creates the requested tag but cannot suggest a trustworthy bump.
Compiling a selected client set helps review without covering every possible downstream client.

A Markdown diff is a different mechanism: reuse `hd doc` output and compare printed declarations.
It looks small, but it loses impl kinds, identity, and machine-readable row/default facts.
Recovering those facts by parsing text would duplicate compiler semantics and defeat the shortcut.
It fails the defaulted-function-value and impl-inference examples unless it grows a typed extractor anyway.

The recommended mechanism projects checked interfaces and compares normalized contracts.
It supports all five workflows with reasons, but pays for explicit type correspondence and conservative rules.
A user-supplied API manifest would add another public language for contracts and can drift from the compiler.
That extension is excluded from the initial mechanism.

| Mechanism | Standalone addition | Row/value/default breaks | Impl-inference break | CI and local tag |
| --- | --- | --- | --- | --- |
| Author chooses exact version; check and tag | Author classifies | Client tests only | Client tests only | Smallest writer; no inferred bump. |
| Diff rendered pages | Usually visible | Text cannot establish all forms | Generic impls can be omitted | Misleading gate. |
| Checked projection and comparison | `minor` | Explicit contract rules | Applicable impl-set comparison | Recommended reader and writer. |

All options add CLI tooling, with no new language syntax, semantic exception, intrinsic, or std item.
The cheaper writer is the fallback if projection maintenance exceeds the budget.
A general theorem prover or compatibility plugin system would be a different, larger project.

## Where the API comes from

Run the ordinary checker on both committed trees with the same compiler and each tree's own resolved dependencies.
Resolve `path` plus `version` dependencies in the [fetched-package view](../../spec/lang/10-modules.md#r-module.workspace.path-version.fetched-version), using the version rather than a neighboring checkout.
Run the dependency resolver offline against known objects; missing sources fail before an implicit fetch can occur.
Keep generated `hd.sum` changes in scratch storage, never either committed source tree.

Use `hd_driver::Output.ifaces` and decoded [folder interfaces](../../crates/hd_resolve/src/iface.rs) as extraction inputs.
`FnSig` already carries named parameters, default-presence bits, result, row, suspension, variadic shape, and generic contracts.
Data, trait, impl, associated-type, and re-export records provide the other typed inputs.

Those records are not already a release snapshot.
`FnSig` does not carry value-default expressions, and the folder's `api_hash` intentionally excludes some private-target impls.
Its broader heads hash serves another reader; neither hash alone defines this tool's contract.
[Documentation impl rendering](../../crates/hd_doc/src/impls.rs) also skips template impls and non-nominal targets.

Add a bounded `hd_driver` projection module and a CLI comparison module after owner approval.
Collect all library folders, follow export chains, expand generated impl contracts, and retain structural/generic impl patterns.
Exclude tests, tasks, block-local declarations, and unreachable private names; do not exclude an impl merely because its declaration lacks `pub`.
Public trait/target applicability and the complete checked type graph determine relevance.

Default/body change notes need a separate normalized syntax fingerprint, not an invented field in today's interface.
Public templates and special compiler-known metadata need explicit extraction coverage.
A coverage failure makes the report `unassessed` before any tag is written.
Do not expose the compiler's cache-blob format as a stable external API.

## Architecture and implementation

### Snapshot data structures

Proposed records use compact indexed vectors internally; sorted public paths exist at the report boundary.
Raw `Ty`, `RowId`, `Symbol`, and `DefId` values never cross the snapshot boundary.
The JSON report is versioned separately from this internal schema.

| Record | Fields |
| --- | --- |
| `Snapshot` | schema; compiler build; Git commit/tree; package repository/directory; dependency resolutions; minimum toolchain; `exports`; `types`; `items`; `impls`; coverage flags. |
| `Export` | public module/path; namespace/kind; local declaration index; source location. Preserve every alias path and shared identity. |
| `TypeNode` | primitive/reference/tuple/function/nominal/trait-value/projection tag; child indexes; permission; nominal identity; arguments; associated bindings; normalized row. |
| `Generic` | owner-relative ordinal; type/row kind; variance; sorted bounds; resolved default. Source binder spelling is display-only. |
| `Callable` | ordered parameter labels/types/default presence; result; row; suspension; receiver; variadic shape; generics; source/default digests. |
| `Data` | kind; ordered fields/payloads with visibility/default/embedded bits; variants; shared enum data; constructor access; resolved promotion paths. |
| `Trait` | member contracts; associated declarations/defaults/bounds; supertraits; generated/public-template metadata. |
| `Impl` | trait identity/arguments; self-type pattern; generics/bounds; associated bindings; method contracts; origin; generated or written; observable applicability. |
| `Finding` | stable change kind; public path; old/new locations and contracts; magnitude; definite/potential evidence; reason. |

Own-package identities correspond across the two releases by shared public paths.
Build a bijection for nominal declarations and verify that aliases preserve the old identity-equality partition.
Do not choose the lexicographically first alias as an identity: adding an earlier alias must not rename a type.
A conflict or removal emits a break before comparing structural children.

Dependency identity follows [host path and compatibility line](../../spec/lang/10-modules.md#r-module.dep.identity).
Exact resolved versions and tree hashes remain provenance, not a false nominal distinction between compatible patches.
Changing the dependency line of an exposed type changes its identity.
Expand transparent aliases; compare nominal types through correspondence; memoize recursive type-pairs to terminate on recursive references.

### Data flow and core comparison

Read Git objects and manifests; resolve/check each source tree; project complete snapshots; compare contracts; print a report or enter the tag writer.
The checker remains authoritative for validity, coherence, generated impls, and type meaning.
Comparison normalizes binder names, rows, bound order, and source-order noise before applying the tables above.
Unknown supported facts are errors; conservative known risks are findings.

```rust
fn compare(old: Snapshot, new: Snapshot) -> Result<Report, Unassessed> {
    require_same_schema_and_compiler(&old, &new)?;
    require_complete_coverage(&old, &new)?;
    let mut out = Report::new(old.provenance(), new.provenance());
    let pairing = pair_nominals_by_shared_public_paths(&old, &new, &mut out);
    verify_alias_identity_partition(&old, &new, &pairing, &mut out);
    let mut types = TypePairs::new(&pairing); // cycle-aware, memoized equality
    for path in sorted_union(old.exports.paths(), new.exports.paths()) {
        match (old.at(path), new.at(path)) {
            (Some(a), None) => out.definite_major(path, "public-path-removed"),
            (None, Some(b)) => classify_addition(b, &old, &new, &mut out)?,
            (Some(a), Some(b)) => {
                if a.kind != b.kind { out.definite_major(path, "kind-changed"); continue; }
                compare_generic_contracts(a, b, &mut types, &mut out)?;
                match a.kind {
                    Function | Method => compare_callable(a, b, &mut types, &mut out)?,
                    Struct | Enum | Newtype | Alias => {
                        compare_data_contract(a, b, &mut types, &mut out)?;
                        compare_constructor_access(a, b, &mut out);
                        compare_promoted_selections(a, b, &mut types, &mut out)?;
                    }
                    Trait => compare_trait_contract(a, b, &mut types, &mut out)?,
                }
                compare_public_template_metadata(a, b, &mut out)?;
                record_body_and_default_change_notes(a, b, &mut out);
            }
            (None, None) => unreachable!(),
        }
    }
    for group in impl_groups_by_trait_and_head(&old, &new) {
        for removed in group.unmatched_old() { out.definite_major(removed, "impl-removed"); }
        for (a, b) in group.paired() { compare_impl_contract(a, b, &mut types, &mut out)?; }
        for added in group.unmatched_new() {
            if proves_fresh_disjoint_pair(added, &old, &new)? {
                out.minor(added, "impl-added");
            } else { out.potential_major(added, "impl-addition-may-change-clients"); }
        }
    }
    out.behavior_assessed = false;
    record_minimum_toolchain_increase(&old, &new, &mut out);
    out.sort_and_deduplicate_findings();
    out.magnitude = maximum_finding_or_patch(&out);
    Ok(out)
}
```

Helper dispatch is defined by the item tables, not by checking example clients.
`compare_callable` first compares generic contracts, then ordered parameter/type/receiver/suspension/variadic facts and normalized rows.
It applies the value-default exception only when those facts match.
Trait comparison adds implementer obligations; data comparison adds construction, pattern, promotion, and generated-impl contracts.

For impl pairing, exact normalized trait/self/generic heads identify retained contracts.
A changed head can appear as a removal plus an addition; deterministic reporting may combine them without lowering the magnitude.
Group structural patterns conservatively, including generic and tuple templates.
If the supported pattern relation cannot prove freshness, emit potential `major`; if a pattern cannot be decoded, return `unassessed`.

### Caching, complexity and determinism

Key each snapshot by compiler/schema, committed source tree, selected package, resolved dependency hashes, and analysis options.
Cache hits reuse decoded checked facts; changed public-template or default fingerprints invalidate their corresponding release notes.
Never use `api_hash` alone or reuse an old report after a dependency selection changes.
No mutable worktree source participates in a committed-tree report.

Export sorting costs `O(E log E)` and type-pair comparison costs `O(P)` visited node-pairs.
Contract scans are linear in projected members and normalized rows.
Impl bucket comparisons can cost `O(I²)` for a large shared generic-head bucket; this is the explicit superlinear step.
Bound comparisons by a work limit; exceeding it reports `unassessed`, with the responsible trait/head, rather than a partial success.

Stable sorted vectors and memoized type-pairs bound retained memory by snapshots plus visited pairs and findings.
Use existing checker parallelism through `--jobs`; comparison starts single-threaded for reproducibility.
Report equality excludes elapsed times and machine-local cache paths.
Incremental stress tests must compare cold and warm report bytes.

## Command and tags

One command, `hd release`, does both the reading and the writing (owner, 2026-10-23):

```sh
hd release                 # read only: print what changed in the public API, and the smallest version it needs
hd release 2.0.0 --dry-run # run every check for 2.0.0, create nothing
hd release 2.0.0           # tag this exact version, after checking it is a large enough bump
hd release -p lint --since 1.4.0 --format json
```

A bare `hd release` only reads: it prints the API report and the smallest version the change allows, as [Bump arithmetic](#bump-arithmetic) computes it, exits 0, may run in a dirty checkout, and creates nothing.
Creating a tag always needs the exact version, because a tag is too easy to create by mistake when the tool picks the number.
`--dry-run` runs the same checks as the versioned call, and writes nothing.
An explicit version that is too small for the change fails, and names the smallest one that would pass.
The tag is local, annotated and unsigned; `hd release` never pushes or publishes.

### Selection and configuration

Both commands select the current library package; `-p NAME` selects exactly one workspace member.
At a multi-member workspace root, require `-p` rather than releasing every member implicitly.
Reject a package with no importable library root for this first version.
Workspace-wide coordinated releases, executables' command-line compatibility, and registry publication are outside this command's checked contract.

`--since VERSION` selects an exact local package-prefixed tag and requires its commit to be an ancestor of pinned HEAD.
Without it, choose the greatest matching version among HEAD's ancestors, as [version selection](../../spec/lang/10-modules.md#r-module.version.pseudo) does.
Include prereleases in that search and order them by [SemVer precedence](../../spec/lang/10-modules.md#r-module.version.order).
Never use lexical tag order or another package's prefix.

No release-specific `hd.toml` keys are introduced.
Existing package/workspace/dependency and toolchain configuration still applies to each committed tree.
CLI selection, baseline, `--format text|json`, and `--jobs N` control only this invocation; invalid combinations are usage errors.
The version is positional; without it the command only reports.

Both commands analyze committed HEAD, never unstaged or staged changes.
The reader may run in a dirty checkout and labels that fact; the writer and its dry-run require a clean repository.
A missing explicit baseline is an error; an absent default baseline is `first-release`.
A failed baseline check is `unassessed`, never permission to pretend this is the first release.

### Bump arithmetic

First aggregate API findings; then validate the author's exact target version.
For stable releases, `major`, `minor`, and `patch` require the corresponding core increment or a larger one.
For `0.y.z`, breaking changes must cross the [compatibility line](../../spec/lang/10-modules.md#r-module.version.line): `0.(y+1).0` or a stable major.
Compatible major-zero additions use a patch bump, so the API magnitude and suggested numeric bump are separate report fields.

For example, `1.4.2` with a break needs at least core `2.0.0`; an addition needs `1.5.0`; an unchanged API needs `1.4.3`.
At `0.4.2`, a break needs `0.5.0`, while an addition or patch needs `0.4.3`.
Compare numeric components, not formatted strings; `1.10.0` follows `1.9.0`.
A target must also be strictly newer than its selected baseline.

Prerelease targets are author-written; never guess suffix increments.
A later prerelease or final release of the same core is accepted only for `patch` API magnitude.
A compatible addition must cross the minor core, except that a later same-core prerelease may carry it before that core is final.
A break still crosses the compatibility line, even between prereleases; the tool does not weaken hd's existing line rule.

A target prerelease with a sufficiently increased core satisfies the corresponding bump, while remaining labeled prerelease.
Reject build metadata: hd's accepted tag grammar allows a prerelease suffix, not a `+metadata` extension.
A first release has no minimum bump; it still needs an author-specified valid version and a successfully checked package.
Behavioral changes can always justify a higher target than the API minimum.

### Output and exit statuses

Text shows package, baseline tag/object, checked HEAD, assessment, each changed public path, reasons, and suggested version or first-release state.
Old/new signatures include rows and labels; source locations identify actual declarations, including a re-export path's target.
Always print that behavior was not assessed and that tests were not run by this command.
A successful writer additionally reports the exact tag and commit, with pushing left to the author.

JSON is newline-delimited records: zero or more `api_change`/diagnostic records followed by one summary.
This proposed schema is independent of internal compiler cache formats.
Unknown future change kinds can be displayed as text by old clients; a new summary schema requires a version increment.

```json
{"kind":"api_change","schema":1,"path":"api.answer","change":"row-changed","magnitude":"major","evidence":"definite","old":"fn() -> i32","new":"fn() -> i32 $ Clock","locations":{"old":{"file":"src/lib.hd","line":1},"new":{"file":"src/lib.hd","line":3}}}
{"kind":"summary","schema":1,"package":"lint","baseline":{"tag":"lint/v1.4.2","commit":"<oid>"},"head":"<oid>","assessment":"complete","api_magnitude":"major","suggested_version":"2.0.0","behavior_assessed":false,"minimum_toolchain_assessed":false,"tag_created":null,"status":0}
```

| Exit | Meaning |
| --- | --- |
| `0` | Report completed, even with major findings; requested version is adequate, or local tag creation succeeded. |
| `1` | Analysis completed but the given version is insufficient/not newer. No tag is created. |
| `101` | Usage, checking, extraction, missing object/dependency, dirty writer tree, or Git transaction failure. No success summary. |

An incomplete operation still emits a final summary with `assessment: "unassessed"`, `status: 101`, and no suggested version.
A first-release summary instead uses `assessment: "first-release"`, `api_magnitude: null`, and status zero if validation succeeds.
A report must not call an unsupported construct compatible.
Example errors explain the action: “cannot check baseline lint/v1.4.2: dependency object is missing; fetch it explicitly.”

### Checked local tag transaction

The tag prefix is the package's repository-relative directory, not the manifest's package name.
At repository root, version `1.5.0` becomes `v1.5.0`; directory `tools/lint` becomes `tools/lint/v1.5.0`.
Do not create a version field, rewrite manifests, or change dependency requirements automatically.
Reject an existing target tag, including one pointing at the same commit.

Pin HEAD, the baseline tag object and peeled commit, and dependency source hashes before analysis.
Read committed blobs through a Git-object source provider, without checkout filters or [archive substitutions](https://git-scm.com/docs/git-archive#ATTRIBUTES).
No package initializer, task, release script, or test body executes.
Missing objects require an explicit author fetch; the commands never fetch or push.

```rust
fn release(request: Request) -> Result<Tag, Error> {
    let pinned = pin_head_package_and_baseline(&request)?;
    require_clean_repository()?; // tracked changes and nonignored untracked files
    let report = check_project_compare(&pinned, request.jobs)?;
    require_complete_or_valid_first_release(&report)?;
    require_adequate_exact_version(&report, request.version)?;
    reject_unversioned_path_dependencies(&pinned.current_manifest)?;
    let name = package_directory_tag(&pinned.package, request.version)?;
    require_absent_ref(&name)?;
    if request.dry_run { return Ok(Tag::Preview(name, pinned.head)); }
    require_clean_repository()?;
    require_head_and_baseline_unchanged(&pinned)?;
    let object = write_unsigned_annotated_tag_object(name, pinned.head, report.digest())?;
    create_ref_transaction_with_guards(
        name, object, Expected::Absent, pinned.head, pinned.baseline_ref
    )?; // verify pinned refs and create tag in one Git ref transaction
    Ok(Tag::Created(name, pinned.head))
}
```

The final [Git ref transaction](https://git-scm.com/docs/git-update-ref) verifies HEAD and the baseline while creating the absent tag ref.
A different HEAD object or changed baseline makes it fail; switching branches at the same checked commit does not invalidate the analysis.
Concurrent worktree file edits cannot alter the pinned committed tree being tagged.
A failed transaction may leave an unreachable tag object, but no partial release ref.

Use argument vectors, validate tag names, and disable hooks per Git invocation with an owned empty `core.hooksPath`.
Disable configured fsmonitor commands during status reads, and reject external source filters rather than executing them.
Do not invoke signing programs or inherit a configured signing step; initial tags are explicitly unsigned and annotated.

Remote URLs and credential helpers are not needed for this local operation.
These constraints exclude npm-style hooks and cargo-release-style remote effects from the security surface.

### Platforms, authority and failure

Git-object paths are case-sensitive identities on every platform.
A source provider avoids Windows checkout casing and symlink traversal; reject unsupported submodules or unavailable source objects as `unassessed`.
Source/dependency reads and scratch cache writes are the reader's effects, with no tracked-file mutation.
The writer additionally writes Git objects and one local tag ref; it grants no capabilities to the analyzed program.

macOS, Linux, and Windows use the same normalized snapshots and JSON ordering.
Neither Node nor wasmtime executes during API extraction; engine choice cannot alter a signature comparison.
No application Wasm or runtime feature is linked by this command.
Package checks can fail on compiler limitations, and the report preserves that failure rather than manufacturing compatibility.

## Extension points

The initial comparator has built-in versioned rules and no user-defined downgrade policy.
CI can consume JSON and require a larger bump or additional downstream tests.
An external consumer cannot turn an incomplete built-in analysis into a successful `hd release` transaction.
Custom suppression, release hooks, plugins, and alternate tag patterns require separate approval because they change trust or compatibility guarantees.

Public user derivations are inputs, not executable release plugins.
Their checked impls and template metadata participate in the snapshot.
Future compiler-owned snapshot schemas can add supported facts without exposing internals as a public cache protocol.
A per-rule override language waits until actual conservative false positives justify its stability and maintenance cost.

## Cost

This is a docs-only proposal; implementation numbers below are budgets, not measured gains.
Literal `wc -l` at `a93482cb9` gives `hd_resolve/src/iface.rs` 2,525 lines and `hd_driver/src/lib.rs` 10,626 lines.
The relevant existing CLI modules are `doc_cmd.rs` 359 lines and `dep_cmd.rs` 975 lines; `hd_doc/src/impls.rs` is 88 lines.
These files remain intact: the proposal replaces **zero** existing code lines and reuses their typed facts and selection helpers.

| Added component | Derivation of proposed allowance | Rust-line budget |
| --- | --- | ---: |
| Checked projection and coverage verifier | Export/type walk 150; callable/data/trait/impl extraction 160; fingerprints/validation 90 | 400 |
| Cross-version identities and normalization | Nominal correspondence 80; recursive type pairs 80; row/binder normalization 60 | 220 |
| Item and impl comparison | Callable/default rules 70; data/promotion rules 80; traits/impl/generics 130 | 280 |
| Git trees, baselines and package selection | Reuse selection; object source 100; ancestry/prefix/manifest validation 80 | 180 |
| Reader/writer output and tag transaction | Flags/JSON/text 90; guarded unsigned tag transaction 90 | 180 |
| Focused unit and Git-fixture test harness | Snapshot deltas 100; client pairs 80; tag/race/status fixtures 120 | 300 |
| Total | No new dependency assumed; stop if a component needs one | **1,760** |

Measured reference sizes are Elm `Diff.hs` 364 lines, Elm `Bump.hs` 171 lines, and Go `apidiff.go` 337 lines.
These are literal source line counts, including comments and blanks, downloaded on 2026-10-23.
They exclude supporting diff/type packages, loaders, and CLI frameworks; they are scope references, not whole-tool cost claims.
Their source hashes and raw downloads are retained with the probe evidence.

This budget deliberately spends more on hd's rows, templates, defaults, and impl inference than a page diff would.
Stop any implementation component above 1.5 times its row's allowance and return for review before extending it.
A source-provider change needing broad driver work also stops for a scoped issue.
Do not hide a new Git library or generic theorem prover inside the allowance.

Compile-time work is two ordinary package checks, projection, and comparison; no benchmark was run for a nonexistent command.
Measure cold/warm time, cache hit counts, projected type-pairs, worst impl bucket, and peak memory on a large package before merging implementation.
Application runtime overhead and emitted Wasm change are expected zero because the command stops before monomorphization/emission; implementation must verify byte-identical builds.
Native CLI binary growth is unmeasured and must be recorded separately.

Keep the comparator and Git writer out of the web compiler dependency graph.
Its expected gzip delta is zero, to be checked by the existing web size gate; the 2 MB cap still applies.
This revision neither changes that artifact nor claims a new size measurement.
A budget violation or extraction coverage gap falls back to a smaller checked tag writer only after owner review.

## Stress test

The [2026-10-24 release stress report](RELEASE_STRESS.md) tests the single-command design against real library histories and typed clients.
It also exercises Git/version edge cases.
It records unresolved prerelease arithmetic without changing this proposal's decisions.

The ten old-library/new-library/client pairs above are executable translations of practical API hazards.
The embedding client ports the `Point`/extra-field witness in Go apidiff's README to hd embedding.
The `Source[T]` case follows hd's bound-inference rule; defaulted callable and enum cases exercise the client forms compatibility tools must preserve.

Elm's exposed-module diff and Go's type correspondence additionally exercise module additions and identity-based matching in the proposed algorithm.
They are source studies, not claims to have run those external tools.

| Adversarial case | Evidence or expected implementation test | Design response |
| --- | --- | --- |
| A function gains `$ Clock` | Actual new-client `missing-requirement` | Definite major. |
| A function gains a defaulted parameter | Actual stored-function `type-mismatch` | Arity wins over direct-call compatibility. |
| An existing parameter gains a default | Actual old/new clients pass both engines | Minor: omitted calls become possible without changing the stored function type. |
| A struct gains a public defaulted field | Actual literal/pattern control passes both engines; embedding client fails | Potential major because downstream promotion can collide. |
| An enum gains a variant | Actual `nonexhaustive-match` | Closed variants are not a compatible extension. |
| A derive disappears | Actual operator `type-mismatch` | Compare generated impls. |
| A function loses `$ Clock` inside an inferred container | Actual nested-function `type-mismatch` | Row shrinking is not universally compatible. |
| Another `Source` impl appears | Actual `ambiguous-type` | Do not classify impl addition by set inclusion alone. |
| A type binder is renamed | Actual old/new clients pass both engines | Normalize ordinal binders. |
| A facade adds an earlier-sorting alias; recursive public types | Planned snapshot tests: same identity and finite pair walk | Preserve aliases and memoize correspondence. |
| Old public type gains private storage or a promoted-name collision | Planned old/new constructor/member clients | Construction visibility and resolved promotion participate. |
| `lint/v1.9.0`, `lint/v1.10.0`, other-package tags, and prereleases | Planned disposable-Git CLI fixtures; version arithmetic model below | Numeric ordering, package prefix, explicit prerelease target. |
| Dirty checkout, missing baseline/dependency, checking error, or an undecodable template | Planned negative CLI fixtures | Informative error; no tag and no patch fallback. |
| HEAD/baseline/target changes before ref creation | Six actual disposable-Git plumbing scenarios, including failures | Verify pinned objects and absent tag together; never retag. |
| Cold versus warm cache, changed dependency body, huge generic impl bucket | Planned deterministic reports/work-limit tests | Provenance keys, behavior caveat, explicit unassessed limit. |

These probes found real counterexamples to two tempting simplifications: all row shrinking is safe, and all impl additions are minor.
The defaulted-field control separates a surviving client from a universal compatibility claim.
A more permissive policy could explicitly exclude downstream embedding conflicts, as Go does; that would need owner approval rather than an undocumented exception.

The current checker accepts both versions of all ten libraries; seven new clients fail for the intended reason.
No release extraction, classification, or tagging implementation is claimed by those results.

## Phasing

Each row is an independently green slice targeted at roughly one hour of agent work.
Split or stop a slice that exceeds that window; no slice silently widens compiler scope.
Tests here are the future implementation checks, not authorization to run full suites for this document.

| Slice | Deliverable | Focused proof |
| --- | --- | --- |
| 1 | Internal snapshot structs and coverage verifier | Known/unsupported interface-kind unit fixtures. |
| 2 | Type graph and nominal correspondence | Recursive types, alias additions, and identity splits. |
| 3 | Callable/default/row comparison | Row addition/removal and stored function-value pairs. |
| 4 | Data and visibility comparison | Struct literals, subset patterns, variants, and promoted paths. |
| 5 | Traits, generics, derived impls and templates | Bound-inference ambiguity and downstream implementations. |
| 6 | Committed-object source and baseline selector | Numeric/prefixed tags and missing-object failures in disposable repos. |
| 7 | bare `hd release` text/JSON and version validation | Exact output, statuses, first release, and cold/warm equality. |
| 8 | `hd release VERSION` guarded transaction | Dry-run creates nothing; valid release creates one annotated ref; races create none. |
| 9 | Footprint and large-package validation | Native tool cost, unchanged program Wasm, web gzip, bounded worst bucket. |

Rollback before acceptance is deleting the isolated reader/writer/projection modules, not changing package version semantics.
After acceptance, output schema and rule behavior require ordinary CLI compatibility review.
The implementation issue must name exact files and conformance fixtures before work starts.

## Recommendation

Recommend **checked interface comparison**, with one **`hd release`** command: a bare call reports, `--dry-run` checks, and `hd release VERSION` creates one checked local tag.
It gives up automatic behavioral certification, publishing, and permissive guesses about unresolved impl relationships.
The next-best mechanism is the smallest checked tag writer with author-selected compatibility.
A rendered-documentation diff should not gate a release.

Approve the two command names, explicit write behavior, typed surface/risk policy, version arithmetic, and local unsigned annotated-tag boundary before implementation.
Also approve the internal projection scope and bottom-up budget; this note grants no permission to edit protected compiler lanes.
Accepted changes would belong in the CLI tier, with diagnostics and conformance cases under a subsequent spec order.
No existing language rule or rule ID is retired by this documentation revision.

## Approval and validation

### Sources and parse log

Primary external sources are linked in each prior-art subsection; compiler sources and exact semantic rules are linked at their use sites.
Probe tools were freshly built from `a93482cb92d52ca2213a2517a173327ae65b76a8` before running these examples.
The frozen CLI hash is `f92d1d9b862a0c1af22e89bca16b72613780a9ec40940c8ba4c2413241f0bad4`;
the parser hash is `fd5602912897d519c2762a9ad42199d2cee2722df0ec05af096559a9ce33aeb1`.

All 30 hd blocks parse and match the 40 versioned source files used by the ten old/new pairs.
The repeated unchanged clients were checked against both libraries, not a copied local declaration.
All 20 library checks and ten old-client checks pass; three new-client controls pass and seven reject with the recorded diagnostics.
The thirteen accepted clients also run on Node and wasmtime: 26 runs, with the expected output and successful assertions.

Per-file formatting, parsing, 40 package checks, and those 26 runs produced 186 successful expected-outcome checks.
Rejected new clients are intentional compatibility witnesses, not passing runtime prototypes.
Proposed commands, extraction records, and Rust-like algorithms are design notation, not implemented features.
Final document links, preserved headings, spec checks, and website build are recorded in the delivery report.

A separate version-arithmetic model passes 31 cases: 24 bump comparisons, six invalid versions, and one prefixed numeric-order selection.
It tests the proposed arithmetic, not an implemented API extractor or Git ancestry selector.
Six disposable-Git transaction scenarios pass on Git 2.54.0, covering success, changed objects, existing tags, and detached HEAD.
A rejected earlier transaction form tried verifying a symbolic HEAD and its branch twice; Git refused it before creating a ref.
