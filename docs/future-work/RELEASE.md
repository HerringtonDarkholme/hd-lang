# Releasing a package

Status: proposal for [#28](https://github.com/HerringtonDarkholme/hd-lang/issues/28), 2026-10-23; nothing here is accepted behavior or specification.

## Recommendation

Recommend `hd release`: compare checked public APIs first, and create a local tag only with an explicit version and `--tag`. Pushing stays a separate Git command. This adds CLI tooling, with no language mechanism or manifest version field.

Today, [versions are Git tags](../../spec/lang/10-modules.md#r-module.version.tag), including [subdirectory prefixes](../../spec/lang/10-modules.md#r-module.version.tag-prefix). [Release compatibility tooling](../../spec/cli/command-line.md#r-cli.tooling.package-later) is deliberately unspecified. At `9c39889b6`, the CLI has no release command. A two-signature probe of `hd doc --format json pkg.answer` produces Markdown plus summary JSON, not typed API records.

## Command and tags

Proposed invocations:

```sh
hd release                              # inspect; change nothing
hd release --since 1.4.0 --format json   # choose the comparison baseline
hd release 1.5.0 --tag                   # validate and create a local tag
```

`-p NAME` selects one workspace member; otherwise use the current package. `--since VERSION` selects its exact local baseline tag. The default is its greatest matching version among HEAD's ancestors.

`--format json` reports baseline/HEAD identities, findings, suggested bump and proposed tag. A version argument without `--tag` previews that release. Reports compare committed HEAD, not uncommitted edits.

Derive the prefix from the package's repository-relative directory, never its package name: `lint/v1.5.0`, or `v1.5.0` at the root. Ignore other packages' tags. No tags means “first release; compatibility unassessed”, requiring an author-selected version. A missing explicit baseline is an error.

Tagging requires a clean repository, a successful check, no existing target tag, and [no unversioned path dependency](../../spec/lang/10-modules.md#r-module.version.no-bare-path-release). Create an annotated tag at the verified HEAD; reject retagging, a version not newer than the baseline, or an insufficient bump.

Missing revisions/dependencies require an explicit fetch. Do not fetch, commit, push, or run release hooks. Tests remain an explicit author step.

## What is compatible

Compare source compatibility for existing clients and implementers. Function bodies, behavioral contracts, and supported compiler versions still need author review; unchanged signatures cannot certify SemVer compatibility.

| Change | Proposed finding |
| --- | --- |
| Remove a public name/re-export, restrict visibility, change types/receiver/suspension, require another argument, or tighten generic bounds | Breaking. Include named parameters and loss of defaults. |
| Enlarge a callable’s own requirement row, tighten row bounds, change a trait signature, or require a new trait member/associated type | Breaking for callers or implementers. |
| Remove/tighten an existing impl or change its associated types; add an impl/method that may change existing overlap or lookup | Breaking or conservatively major until proved compatible. Include derived and generic impls. |
| Add an enum variant or required constructor field, remove/change a public field, or prevent existing struct literals | Breaking: exhaustive matches and struct literals are clients too. |
| Add a public item/impl that cannot affect existing lookup | Compatible addition. Documentation/order changes do not change API. |

Normalize requirement rows: `$ A + B` equals `$ B + A`. Removing requirements from an ordinary function is compatible under [row subsumption](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume); changing a trait contract must also preserve existing implementations. Do not treat every impl addition or row edit as harmless.

For example, these alternative versions of one exported function both check, but the second adds an obligation to existing callers:

```text
pub fn answer() -> i32: +42
```

```text
use std.time.Clock

pub fn answer() -> i32 $ Clock: +42
```

For stable versions, recommend major for breaking changes, minor for compatible additions, and patch when API is unchanged. For `0.y.z`, use a minor bump for breaks and a patch bump for compatible changes, preserving hd's [compatibility line](../../spec/lang/10-modules.md#r-module.version.line).

Prereleases require an exact version; do not guess suffix increments. Allow later same-core prereleases for compatible API changes; breaking changes still cross the compatibility line. Authors may choose a higher bump for behavioral changes.

## Where the API comes from

Check both committed trees with the same compiler and their own dependency manifests. Project public names/re-exports, resolved signatures, bounds, default presence, normalized rows, data shape, and externally relevant impls/associated types. Canonical declaration paths replace run-local IDs; source order, spans and rendered text are irrelevant. If either tree cannot be checked, report “unassessed” and refuse tagging.

The [interface model](../../crates/hd_resolve/src/iface.rs) already carries signatures, default presence and rows; its exported view includes every impl. [Documentation rendering](../../crates/hd_doc/src/impls.rs) skips generic templates and non-nominal targets, so diffing its pages would miss API. Default expressions and behavior still need review. Add a release projection, not a public cache-file format.

| Mechanism | Benefit | Cost or gap |
| --- | --- | --- |
| Checked interface projection — recommended | Reuses resolved types and impls; consistent text/JSON findings | Requires a bounded compiler-facing extractor and conservative compatibility rules. |
| Diff rendered `hd doc` pages | Small initial implementation | Formatting noise and omitted impls; unsuitable as a compatibility gate. |
| Author chooses a bump; compile selected downstream clients | Smallest alternative; no API extractor | Useful evidence, but untested clients receive no bump analysis. |

[SemVer](https://semver.org/spec/v2.0.0.html) requires an explicit public API and leaves behavior to maintainers. [Cargo's compatibility guide](https://doc.rust-lang.org/cargo/reference/semver.html) distinguishes definite and possible breaks, including trait and method changes. [cargo-release](https://github.com/crate-ci/cargo-release/blob/main/docs/reference.md) separates preview from execution; it also supports publishing packages.

## Approval and validation

Owner approval comes before a CLI spec order and a scoped implementation issue. That order would define release diagnostics, exit statuses and Git-fixture tests; it would preserve the existing tag and language rules. No code or spec change is authorized by this note.

The two hd blocks were parsed and checked independently as library packages. Validation used a frozen parser/CLI from `bc3ffb05a3` plus #526's runtime-only change; parser and requirement-row syntax were unchanged at this note's base. These checks establish example validity, not an implemented release command.
