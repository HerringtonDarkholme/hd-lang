# Dependencies Through Version Control: Survey And Options

Status: design exploration, 2026-09-29; nothing here is decided or in the
specification. It explores the owner's direction of 2026-09-28, quoted
below, and reviews these records and sections:

- [Packages](PACKAGES.md), its draft sections and
  [owner decisions](PACKAGES.md#owner-decisions) 1 to 14;
- [Package Manifest](../spec/10-modules.md#package-manifest),
  [Use Roots](../spec/10-modules.md#use-roots),
  [Test Modules](../spec/10-modules.md#test-modules), and
  [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package);
- [Dependency Cycles](DEPENDENCY_CYCLES.md#owner-decisions) DC2 (package
  cycles are forbidden);
- [Durable Replay decision 9](DURABLE_REPLAY.md#owner-decisions) (code
  identity covers all transitive dependencies).

> **Owner direction (2026-09-28).** hd has no package registry. Follow Go
> and manage dependencies through GitHub and other version control hosts.

## Owner Decisions

Decided 2026-09-29.

1. **DEP1: option A, Go modules in hd spelling.** There is no registry.
   - The host path lives only in `hd.toml`; source keeps `dep.<name>`.
   - Versions are git tags, and the tag is the only version (no
     `version =` field).
   - Resolution is minimal version selection.
   - Integrity comes from an `hd.sum` file.

   This overturns PACKAGES decisions 2, 3 and 11.
2. **DEP2:** two major versions of one repository are told apart by the
   requirement line (two keys), with no `/vN` path suffix.
3. **DEP3:** the manifest stays `hd.toml` (TOML).
4. **DEP4: integrity is `hd.sum` only.** This is a grassroots project with
   no paid servers. A checksum log or a caching proxy is added only if it
   needs no infrastructure or can reuse free public infrastructure.
5. **DEP5:** private repositories use git's own credentials (helpers and
   SSH keys) plus a private-path pattern. hd never stores credentials.
6. **DEP6:** one repository may hold several packages, using prefixed tags
   such as `lint/v1.2.0`.
7. **DEP7: accepted, with phasing.**
   - **Must have first:** workspaces (committable) and untagged commits
     through pseudo-versions.
   - **Later:** the `hd api diff` compatibility check at release and
     upgrade, vendoring (`hd vendor`), and local-path patches hashed into
     the code identity.
   - Hosts: known hosts, plus a `.git` suffix for others, with no meta-tag
     discovery.

## Contents

1. [Problem](#problem)
2. [Survey](#survey)
3. [Design Questions](#design-questions)
4. [Option A: Go Modules In hd Spelling](#option-a-go-modules-in-hd-spelling)
5. [Option B: Tags With Ranges And A Solver](#option-b-tags-with-ranges-and-a-solver)
6. [Option C: Module Paths In Source](#option-c-module-paths-in-source)
7. [Option D: URL Plus Hash, No Resolution](#option-d-url-plus-hash-no-resolution)
8. [Comparison](#comparison)
9. [Ranking By Cost Order](#ranking-by-cost-order)
10. [Recommendation](#recommendation)
11. [Questions For The Owner](#questions-for-the-owner)
12. [Sources](#sources)
13. [Parse Log](#parse-log)

## Problem

Where does hd get a dependency's source, how does it name it, and how does
it pick a version, now that there is no registry? The answer must keep
builds reproducible and must not change the language more than needed.

### What hd Has Today

The spec keeps the package model small and leaves the rest to tooling:

| Rule | Text today |
| --- | --- |
| [`module.manifest.file`](../spec/10-modules.md#r-module.manifest.file) | A package has an `hd.toml` manifest. |
| [`module.manifest.tooling`](../spec/10-modules.md#r-module.manifest.tooling) | Schema, resolution, lockfile, and versions are tooling work. |
| [`module.manifest.dependency`](../spec/10-modules.md#r-module.manifest.dependency) | The manifest maps each dependency name to one resolved package. |
| [`module.root.dep-prefix`](../spec/10-modules.md#r-module.root.dep-prefix) | Source names a dependency only as `dep.<name>`. |
| [`module.package.identity-part`](../spec/10-modules.md#r-module.package.identity-part) | Resolved package identity is part of declaration identity. |
| [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package) | The package graph is acyclic. |
| [`module.test.dependency`](../spec/10-modules.md#r-module.test.dependency) | Only test code may use a test dependency. |
| [`module.test.cyclic-dependency`](../spec/10-modules.md#r-module.test.cyclic-dependency) | A test dependency that depends back is usable only from `tests/`. |

[PACKAGES.md](PACKAGES.md) drafts a registry design on top of that. Its
decisions that a registry-free design touches:

| Decision | Content | Registry assumption |
| --- | --- | --- |
| 1 | One version per compatibility line; lines coexist | none |
| 2 | Caret ranges, PubGrub-style solver | the registry serves each version's manifest separately ([§7](PACKAGES.md#7-distribution)) |
| 3 | Names are `owner/name` | a registry verifies owners |
| 8 | Each `0.MINOR` is its own line | none |
| 9 | No git or path dependencies in published packages | a publish step exists |
| 10 | Toolchain minimum plus an optional root pin | none |
| 11 | A published package holds sources and the interface file | a publish step exists |
| 14 | A public checksum log once a public registry exists | the trigger is a registry |

The registry also enforced the checked compatibility rule at publish
([§3.3](PACKAGES.md#33-the-checked-compatibility-rule)). Without a registry,
something else must run that check.

The key fact for the language: source never names where a dependency comes
from. A `use` line says `dep.json`, and the manifest maps `json` to a
package. So most of this question is tooling, and the spec changes only if
an option moves identity into source.

### Use Cases

Every option is shown against the same seven cases:

| ID | Case |
| --- | --- |
| U1 | Depend on a public library at `github.com/acme/json`. |
| U2 | Use two majors of one library at once, during a migration. |
| U3 | Develop against a local checkout or a fork of a dependency. |
| U4 | Depend on a private company repository. |
| U5 | Use a test-only dependency from a `tests:` block. |
| U6 | Depend on an untagged commit, such as an unreleased fix. |
| U7 | Rebuild the same program years later, for durable replay code identity. |

### Is It Core?

It waits on no open core decision. The use model, package identity, and
the package cycle rule are all decided. Only option C changes the
language; the others change tooling and the prose of `module.manifest.*`.

## Survey

### Comparison Table

| Tool | Identity | Version source | Resolution | Integrity | Local override | Several majors |
| --- | --- | --- | --- | --- | --- | --- |
| Go modules [1] | module path, `github.com/owner/repo[/sub][/vN]`, written in every import | semver tags `vX.Y.Z`; pseudo-versions for commits | MVS: the largest minimum anyone requires [1][3] | `go.sum` hashes; `sum.golang.org` log; `proxy.golang.org` by default [1][4] | `replace`, `go.work`, `vendor/` [1] | yes, `/vN` makes a new path [1] |
| Deno, HTTP imports [6] | full URL in source | in the URL | none: "one exact version" per URL [6] | `deno.lock` hashes | import map | any number |
| Deno, JSR [6][7] | `jsr:@scope/name` | registry | semver ranges, deduplicated [6] | registry; immutable uploads [7] | import map | per range |
| SwiftPM [8][9] | git URL | git tags | ranges (`from:`, `upToNextMinor`, `exact`), branch, revision [8] | `Package.resolved` pins | local path, edit mode | no, one per identity |
| Nix flakes [10] | flake URL, `github:owner/repo/ref` | git ref | none; `follows` shares an input [10] | `flake.lock`: `rev` plus `narHash` | `--override-input` | any number |
| Zig [11] | `name` plus `fingerprint` in the package | none used: URL plus hash | none | `hash` is identity: packages "come from a `hash`" [11] | `path` dependency | any number |
| Gleam [12] | Hex name, or git URL plus `ref` | Hex registry, or a commit | Hex ranges | `manifest.toml` | `path` | no |
| Cargo git deps [13] | git URL; crate found by its `Cargo.toml` | the dependency's `Cargo.toml` | ranges; git commit locked in `Cargo.lock` [13] | `Cargo.lock` | `[patch]` | per semver line |

### Notes

**Go.** A module path names both the source location and the identity.
Majors 2 and up need a `/vN` suffix, so each major is a separate module
[1]. A commit without a tag gets a pseudo-version such as
`v0.0.0-20191109021931-daa7c04131f5`, which sorts by time [1]. MVS
"uses the oldest version available that meets the requirements", so the
manifests alone fix the build and "the redundancy" of a separate lock goes
away [3].

**Go integrity.** `go.sum` records a hash of each module's zip and of its
`go.mod` [1]. The public checksum database is a transparency log, so every
user sees the same hash for one version [1][4]. `GOPRIVATE` patterns skip
the proxy and the log for private modules [1]. Go 1.24 added `GOAUTH` for
private fetches [2].

**Go toolchain.** The `go` line is a required minimum since Go 1.21; the
`toolchain` line suggests a newer toolchain, which the `go` command can
download [1][5].

**Go cycles.** Module requirement cycles are allowed: "Our algorithms must
not assume the module requirement graph is acyclic" [3]. Only package
import cycles are errors. hd is stricter: its package graph is acyclic.

**Deno.** Deno started with URLs in source and retreated. Its post "What we
got wrong about HTTP imports" names three costs [6]:

| Cost | Deno's words |
| --- | --- |
| Verbosity | "Long URLs clutter codebases" |
| Duplication | "HTTP imports lock you in to just one exact version", so "several variants of the same library" load |
| Reliability | modules "hosted on random websites or personal servers, leading to uptime issues" |

Deno answered with import maps (an alias table, like `dep.<name>`) and the
JSR registry. HTTP imports still work [6].

**SwiftPM.** A dependency is a git URL plus a version requirement read
against the repository's tags; `upToNextMajor` is the recommended form
[8][9]. Resolution reads the manifest at each candidate tag.

**Nix flakes.** Inputs are URLs such as `github:NixOS/nixpkgs`. The lock
records a commit and a `narHash` of the source tree [10]. There is no
version resolution; `follows` makes one input reuse another's pin [10].

**Zig.** A dependency is a `url` and a `hash`, and the hash is the identity:
the URL is one mirror [11]. There is no resolver and no registry. The
`minimum_zig_version` field is "currently advisory only" [11].

**Gleam.** Registry packages use Hex version ranges. Git dependencies name
a `ref`, and the docs say "Always prefer a commit sha reference" [12].

**Cargo.** A git dependency may name a branch, tag, or `rev`. Cargo locks
the commit and updates it only on `cargo update` [13]. crates.io rejects a
published crate that has git or path dependencies [13].

**Security incident.** In 2021 someone published `github.com/boltdb-go/bolt`,
a typosquat of `boltdb/bolt`, with a backdoor. After the Go module mirror
cached it, the author rewrote the git tag to clean code. The mirror kept
serving the cached backdoor for about three years [14].

### Takeaways

1. **An alias table in the manifest is where every retreat ends.** Deno
   added import maps after URL imports; Go keeps paths in source but pays
   with `/vN` edits in every import. hd's `dep.<name>` already is an alias
   table, so source does not have to change.
2. **Without a registry, resolution cost depends on the algorithm.** MVS
   reads only the manifests of versions someone names. A range solver reads
   manifests of many candidate tags, which a registry index used to serve
   cheaply.
3. **A checksum log gives consistency, not safety.** It makes every user
   see the same bytes, which defeats targeted serving and later tag
   rewrites. It cannot tell that the first bytes were malicious, as the
   `boltdb-go` case shows [14].

## Design Questions

Each question below gives Go's answer, the alternatives, and what fits hd.
The options later combine these answers.

### Identity

| | Answer |
| --- | --- |
| Go | The module path, such as `github.com/acme/json`, is the identity and the fetch location. For `github.com`, the first three elements name the repository; more elements name a subdirectory [1]. Other hosts use an HTML `go-import` tag or a `.git` qualifier [1]. |
| Alternatives | (a) path in the manifest, alias in source; (b) path in source, as Go does; (c) hash as identity, URL as a mirror, as Zig does. |
| hd fit | The manifest key stays the `NAME` of `dep.NAME`, so (a) needs no language change. The dependency's own manifest states its path, so a fork fetched under the wrong path is rejected. |

A `use` line cannot hold a host path today. `use github.com/acme/json.Encoder`
is a syntax error in the reference parser. The dotted form
`use dep.github.com.acme.json.Encoder` parses, but it cannot tell where the
repository path ends and the module path begins.

### The Manifest File

| | Answer |
| --- | --- |
| Go | `go.mod`, a line format with `module`, `go`, `toolchain`, `require`, `replace`, `exclude`, `retract`, `tool` [1][2]. There is no version field: the tag is the version. |
| Alternatives | (a) keep `hd.toml`, TOML with a closed schema; (b) a new `hd.mod` line format. |
| hd fit | [`module.manifest.file`](../spec/10-modules.md#r-module.manifest.file) names `hd.toml`, and PACKAGES chose TOML for tool edits. Executables and test dependencies already need tables. |

### Versions

| | Answer |
| --- | --- |
| Go | Semver tags `vX.Y.Z`; a subdirectory module uses a prefixed tag such as `gopls/v0.4.0` [1]. Majors 2 and up need a `/vN` path. An untagged commit gets a pseudo-version [1]. |
| Alternatives | (a) the major in the path, as Go does; (b) the line in the requirement, as PACKAGES does (`json@2.1.0`), with tags `v2.x.y` in one repository; (c) no versions, only hashes. |
| hd fit | Go needs `/vN` because paths appear in source. hd source says `dep.json`, so (b) keeps two lines apart with two manifest keys. |

### Resolution

| | Answer |
| --- | --- |
| Go | MVS: the build list takes, per module, the largest minimum any reached manifest states [1][3]. No lock of versions is needed. |
| Alternatives | (a) MVS; (b) caret ranges and a PubGrub solver [15], PACKAGES decision 2; (c) no resolution: every pin is exact. |
| hd fit | Decision 2 assumed a registry that serves manifests. Over git, a solver must fetch the manifest at many candidate tags. MVS fetches only named versions. |

### Integrity

| | Answer |
| --- | --- |
| Go | `go.sum` hashes each module tree and its `go.mod`; `sum.golang.org` is a public transparency log; `proxy.golang.org` caches modules and is the default `GOPROXY` [1][4]. |
| Alternatives | (a) a committed sum file only: trust on first use per project; (b) plus a public checksum log; (c) plus a caching proxy, which also survives deleted repositories. |
| hd fit | PACKAGES' tree hash already matches Go's `h1:` idea: the same hash for a checkout and an archive. A log or proxy is a service to run, not a registry: nothing is published to it. |

### Local Development

| | Answer |
| --- | --- |
| Go | `replace` in the main module only; `go.work` for several local modules, usually not committed; `go mod vendor` copies dependencies into `vendor/` [1]. |
| Alternatives | PACKAGES' root-only `[patch]` equals `replace`. Workspaces: Cargo-style committed `[workspace]` with one resolution, or Go-style local-only. Vendoring: yes or no. |
| hd fit | `[patch]` carries over unchanged. Vendoring matters more without a registry: a deleted repository otherwise breaks every fresh build. |

### Private Repositories And Auth

| | Answer |
| --- | --- |
| Go | `GOPRIVATE` patterns skip the proxy and the checksum log; credentials come from git, `.netrc`, or `GOAUTH` [1][2]. |
| Alternatives | (a) delegate to `git` and its credential helpers, never read a credential file; (b) an hd credential store. |
| hd fit | (a) keeps credentials out of the repository and out of hd. A private-pattern setting keeps private paths from leaking to a public log. |

### Test And Dev Dependencies

| | Answer |
| --- | --- |
| Go | Test-only imports live in `_test.go` files; since Go 1.17 a module graph prunes dependencies' test-only requirements [1]. `tool` directives track executables [2]. |
| hd fit | [`module.test.dependency`](../spec/10-modules.md#r-module.test.dependency) and `[test-dependencies]` carry over. As in PACKAGES [§4.1](PACKAGES.md#41-algorithm), a dependency's test dependencies never enter selection. With MVS, the root always selects itself, so a test dependency that depends back sees the root package; T24 still limits it to `tests/`. |

### Toolchain And std Versions

| | Answer |
| --- | --- |
| Go | `go 1.23.0` is a required minimum; `toolchain` suggests a newer one, which the `go` command can fetch [1][5]. std ships with the toolchain. |
| hd fit | PACKAGES decision 10 already matches: `hd = "0.9.0"` minimum, `[toolchain] pin` in the root. No change. |

### Security

| Threat | What helps | What does not |
| --- | --- | --- |
| Typosquat path (`boltdb-go/bolt`) | The full path shows in the manifest diff; a near-miss warning in `hd add` | A checksum log: it faithfully records the malicious first version [14] |
| Tag rewritten after release | The committed sum file for existing users; a public log for new users | Nothing, without either |
| Host serves different bytes to different users | A public log | A per-project sum file alone |
| Deleted or renamed repository | A caching proxy, or vendoring | A sum file: it detects, not restores |
| Compromised maintainer releases a new tag | MVS: nobody gets it until they upgrade [3] | Ranges: the next resolution may pick it |

### Reproducibility And Code Identity

Durable replay's code identity covers "the entry module, all transitive
dependencies, and the compiler's semantic version"
([decision 9](DURABLE_REPLAY.md#owner-decisions)). A dependency's part of
that identity is its path, its version, and its tree hash. A `[patch]` to a
local path has no recorded hash, so the build must hash the patched tree.

## Option A: Go Modules In hd Spelling

Go's model, with hd's existing spelling. Identity is the host path, versions
are git tags, selection is MVS, and a sum file holds hashes.

```toml
# hd.toml
[package]
id = "github.com/acme/invoice"
hd = "0.9.0"

[dependencies]
json = "github.com/acme/json@2.1.0"          # U1
json_v1 = "github.com/acme/json@1.9.0"       # U2: a second line, second key
billing = "git.example.com/shop/billing.git@0.4.2"   # U4: private host
patchfix = "github.com/acme/pdf@0.4.1-0.20260912081500-3f2c9e1a7b6d"   # U6: pseudo-version

[test-dependencies]
fixtures = "github.com/acme/test_fixtures@0.2.0"   # U5

[patch]
"github.com/acme/json" = { path = "../json" }      # U3, root only
```

```plain
# hd.sum, written by hd; one line per tree and per manifest (Go's go.sum shape)
github.com/acme/json 2.1.0 tree:sha256:4b1e9f0c2d3a5b6c7d8e9f0011223344
github.com/acme/json 2.1.0/hd.toml tree:sha256:9a8b7c6d5e4f30211a2b3c4d5e6f7081
```

The source is the same as today:

```text
use dep.json.{Value, Encoder}
use dep.json_v1.{Value as OldValue}
use dep.billing.invoice.{Invoice}

pub fn upgrade(old: OldValue) -> Value:
    return Encoder.from_v1(old)

tests:
    use dep.fixtures.{sample_invoice}

    it("round-trips an invoice"):
        assert_equal(upgrade(sample_invoice()), sample_invoice())
```

Rules it adds or changes:

| Area | Rule |
| --- | --- |
| Identity | A package is `(path, line)`. `[package] id` states the path, and a fetched manifest must match the requested path. |
| Versions | Tags `vX.Y.Z`, or `sub/vX.Y.Z` for a subdirectory package. `[package] version` is dropped: the tag is the version. |
| Lines | A line is the major, or `0.MINOR` (decision 8). No `/vN` path. |
| Selection | MVS per `(path, line)`: the largest minimum stated by any reached manifest. The root's `[patch]` then replaces sources. |
| Hashes | `hd.sum` holds a tree hash per version and per manifest. A mismatch is an error, never a warning. |
| Compatibility | `hd release vX.Y.Z` runs `hd api diff` against the previous tag of the line, then tags. `hd get -u` reruns the diff before taking a newer version in a line. |
| Cycles | The package cycle rule counts `(path, line)` nodes. A cycle is an error even where Go would allow it. |

Soundness: MVS picks one version per line, so declaration identity stays
one per `(path, line)`, as decision 1 wants. Coherence runs over the
selected interfaces as before.

Go spellings this option does not take, and why:

| Go spelling | hd spelling here | Reason |
| --- | --- | --- |
| `go.mod` line format | `hd.toml` | The spec already names `hd.toml`; TOML tables hold executables and test dependencies. |
| `/v2` in the path | `@2.1.0` in the requirement | Source never shows the path, so no import needs rewriting. |
| Implicit package name from the path | explicit manifest key | No action at a distance: the key a reader sees is the name `dep.` uses. |
| `go.work` uncommitted | `[workspace]` as PACKAGES drafts it | Open: see question DP10. |

## Option B: Tags With Ranges And A Solver

Keep PACKAGES' resolution (decision 2) and swap the registry for git
repositories, as SwiftPM does.

```toml
[dependencies]
json = { git = "https://github.com/acme/json", version = "2.1.0" }       # U1: ^2.1.0
json_v1 = { git = "https://github.com/acme/json", version = "1.9.0" }    # U2
billing = { git = "ssh://git@git.example.com/shop/billing", version = "0.4.2" }   # U4
patchfix = { git = "https://github.com/acme/pdf", rev = "3f2c9e1a7b6d4c58e0f1a2b3c4d5e6f708192a3b" }   # U6

[test-dependencies]
fixtures = { git = "https://github.com/acme/test_fixtures", version = "0.2.0" }
```

```text
use dep.json.{Value, Encoder}
use dep.json_v1.{Value as OldValue}
```

Rules it adds or changes:

| Area | Rule |
| --- | --- |
| Identity | The normalized repository URL plus the line. |
| Candidates | Every tag `vX.Y.Z` in the repository is a candidate. The solver reads `hd.toml` at each candidate it tries. |
| Selection | PubGrub over caret ranges, preferring locked versions. |
| Lock | `hd.lock` records each chosen version, commit, and tree hash, as in [PACKAGES §5](PACKAGES.md#5-lockfile). |
| Compatibility | As in option A, but the check matters more: a range admits versions the author never tested. |

Cost: a fresh resolution fetches tags and manifests from every candidate
repository. A proxy that serves manifests per tag would bring back the
registry's index in all but name.

## Option C: Module Paths In Source

Go's model in full: the path appears in every `use`, and the manifest holds
only versions. It answers the brief's `use github.com/acme/json.Encoder?`
directly.

```toml
[requires]
"github.com/acme/json" = "2.1.0"
"github.com/acme/json/v1" = "1.9.0"
```

```text
use "github.com/acme/json".{Value, Encoder}         # hypothetical syntax
use "github.com/acme/json/v1".{Value as OldValue}   # hypothetical syntax
use github.com/acme/json.Encoder                    # hypothetical syntax
```

Rules it adds or changes:

| Area | Rule |
| --- | --- |
| Syntax | A new use root: a string or slash path naming a module. |
| Use roots | [`module.root.dep-prefix`](../spec/10-modules.md#r-module.root.dep-prefix) and the `dep.<name>` root go away, or become a second form. |
| Majors | Two lines of one repository need two paths, so `/vN` comes back, as in Go. |
| Everything else | As option A. |

Cost: every major upgrade edits every `use` of the package, as Go's `/v2`
does. Deno found long paths in source to be clutter and added an alias
table back [6].

## Option D: URL Plus Hash, No Resolution

The radical simplification, after Zig and Nix. A dependency is a URL and a
content hash. There are no versions to compare and nothing to resolve.

```toml
[dependencies]
json = { url = "https://github.com/acme/json/archive/3f2c9e1a7b6d.tar.gz", hash = "sha256:4b1e9f0c2d3a5b6c7d8e9f0011223344" }
json_v1 = { url = "git+https://github.com/acme/json#8d0c2e41a9f3", hash = "sha256:9a8b7c6d5e4f30211a2b3c4d5e6f7081" }
billing = { url = "git+ssh://git@git.example.com/shop/billing#1c2d3e4f5a6b", hash = "sha256:0c5d2f1e7a9b3c4d5e6f708192a3b4c5" }

[override]
"sha256:77aa00bb11cc22dd33ee44ff55006611" = "json"   # unify a transitive copy with ours, like Nix follows
```

```text
use dep.json.{Value, Encoder}
use dep.json_v1.{Value as OldValue}
```

Rules it adds or changes:

| Area | Rule |
| --- | --- |
| Identity | The tree hash. The URL is a mirror; any URL that yields the hash is the same package. |
| Adding | `hd fetch --save URL` downloads, hashes, and writes the entry, as `zig fetch --save` does. |
| Transitive | Each dependency's own manifest pins its dependencies by hash. Equal hashes are one package; different hashes are different packages. |
| Unifying | The root `[override]` maps a transitive hash to one of its own keys. |
| Upgrades | None built in. Moving to a newer commit means fetching a new URL. |
| Compatibility | No line to check against: `hd api diff` runs only when the user asks. |

Soundness: two libraries that pin two commits of `json` bring two `Value`
types. They do not unify without an override, so passing one library's
value to the other fails with a type error. That is Rust's "expected
`Value`, found `Value`" problem, at every commit instead of every major.

## Comparison

| | A: Go in hd spelling | B: ranges and solver | C: paths in source | D: URL plus hash |
| --- | --- | --- | --- | --- |
| U1 public library | one line | one line | one line plus a `use` path | URL plus hash |
| U2 two majors | two keys | two keys | two `/vN` paths | two hashes; no line concept |
| U3 local checkout | root `[patch]` | root `[patch]` | root `[patch]` | `path` entry or `[override]` |
| U4 private repo | git auth, private pattern | git auth | git auth, private pattern | git auth |
| U5 test dependency | unchanged | unchanged | a path in the `tests:` block | unchanged |
| U6 untagged commit | pseudo-version | `rev` pin | pseudo-version | the normal case |
| U7 code identity | path, version, tree hash | lock: commit and tree hash | as A | the hash itself |
| Duplicate copies of a library | one per line | one per line | one per line | one per hash |
| Resolution cost over git | manifests of named versions | manifests of many candidate tags | as A | none |
| Lock file | none; `hd.sum` hashes only | `hd.lock` | none; `hd.sum` | none; hashes in `hd.toml` |
| Upgrade | `hd get -u`, checked by `hd api diff` | `hd update`, checked | as A | manual refetch |
| Agent-writability | a path and a version; `hd get` writes the sum | a URL and a version | the path in every `use` | needs a tool to compute the hash |
| Human readability | the version is visible | the version is visible | the path is visible at the use | a hash, with no version |
| Implementation cost | 7 mechanisms | 8 mechanisms | 7, plus a syntax change | 4 mechanisms |
| Evolution | a proxy and log can be added later without changing files | same | same | adding versions later changes the manifest format |

Mechanisms counted: A has tag mapping, pseudo-versions, MVS, `hd.sum`,
`[patch]`, workspace, and vendoring. B replaces MVS with a solver and adds
tag listing and a lock. D has fetch-and-hash, hash deduplication,
`[override]`, and vendoring.

PACKAGES decisions each option overturns:

| Decision | A | B | C | D |
| --- | --- | --- | --- | --- |
| 1 one version per line | keeps | keeps | keeps; spelled `/vN` | overturns: one per hash |
| 2 ranges and solver | overturns: MVS | keeps | overturns: MVS | overturns: no resolution |
| 3 `owner/name` | overturns: host path | overturns: URL | overturns: host path | overturns: hash |
| 6, 7 compatibility classes | keep; checked at release and upgrade | keep | keep | moot: nothing is compared |
| 8 `0.MINOR` lines | keeps | keeps | keeps; needs a path spelling | moot |
| 9 no git or path deps when published | reframed: no publish step; a release rejects path deps | reframed | reframed | overturns: every dep is a URL |
| 11 published contents | overturns: the tag's tree is the package | overturns | overturns | overturns |
| 14 checksum log with a registry | re-asked (DP8) | re-asked | re-asked | moot: the hash is in the manifest |
| 4, 5, 10, 12, 13 | keep | keep | keep | keep |

## Ranking By Cost Order

The [Design Cost Order](../AGENTS.md#design-cost-order) ranks language
changes. Three options change no language rule:

| Option | Syntax | Rule exception | Intrinsic | Core library | Other changes |
| --- | --- | --- | --- | --- | --- |
| A | none | none | none | none | prose of `module.manifest.*`; 7 tooling mechanisms |
| B | none | none | none | none | prose of `module.manifest.*`; 8 tooling mechanisms |
| C | new use-root form | `dep.<name>` root rules change | none | none | as A |
| D | none | none | none | none | prose of `module.manifest.*`; 4 tooling mechanisms |

Ranking, most favored first: **D, A, B, C**. A, B, and D tie on the cost
order, and the tie breaks on the count of mechanisms. C is last because it
is the only syntax change.

D's low count comes from dropping use cases. It has no upgrade path and no
compatibility line, and it allows a copy per commit, which splits types.

## Recommendation

**Recommendation: option A**, Go's model in hd's existing spelling.

- It follows the owner's direction: Go's identity, tags, pseudo-versions,
  MVS, sum file, `replace`, and vendoring.
- It changes no language rule. `use dep.json.{Value}` stays as it is.
- It is the cheapest option that keeps all seven use cases. D is cheaper
  but loses upgrades and splits types.
- MVS suits git better than a solver. It reads only the manifests of named
  versions, while a solver would need the registry index that no longer
  exists. This is the new evidence for reopening decision 2.
- hd can check MVS's trust assumption. Interface files make
  `hd api diff` a local check, at release and at every upgrade.
- A new tag reaches nobody until someone upgrades, which limits a hijacked
  maintainer account.

What it gives up: ranges let a library say "not 2.3.0", which MVS cannot;
Go's answer is `exclude` and `retract` [1]. It also drops the registry's
enforced compatibility check. The author-side check can be skipped by
tagging by hand, and the upgrade-time check is the backstop.

Next best: option B, if the owner wants to keep decision 2. It costs one
more mechanism, a slower fresh resolution, and a lock file.

## Questions For The Owner

**DP1. Where is a dependency's host path written?** Effect: whether
source changes when a library moves or a major changes.
(a) in the manifest; source keeps `dep.<name>`. (b) in every `use`, as Go
does (a syntax change). (c) nowhere; the hash is the identity, as Zig does.
**Recommendation: (a).**

```text
use dep.json.{Value}   # hd.toml: json = "github.com/acme/json@2.1.0"
```

**DP2. How are two majors of one repository told apart?** Effect: whether
a major upgrade edits paths. (a) the line in the requirement, tags
`v2.x.y` in one repository. (b) a `/vN` path suffix, as Go does.
**Recommendation: (a).** Source never shows the path, so `/vN` buys
nothing.

```text
use dep.json_v1.{Value as OldValue}   # json_v1 = "github.com/acme/json@1.9.0"
```

**DP3. Keep `hd.toml`, or adopt a go.mod-like `hd.mod`?** Effect: the
file every agent edits. (a) keep `hd.toml` with a closed schema. (b) a
line format. **Recommendation: (a).** The spec names it, and TOML
tables hold executables.

```text
use dep.fixtures.{sample_invoice}   # [test-dependencies] stays a TOML table
```

**DP4. Is the git tag the only version?** Effect: whether a manifest
`version` can disagree with the tag. (a) drop `[package] version`; the tag
is the version, as in Go. (b) keep both and require them to match.
**Recommendation: (a).**

```text
use dep.json.{Encoder}   # json 2.1.0 is the commit tagged v2.1.0
```

**DP5. MVS, or keep PubGrub over tags?** Effect: whether a new release
changes anyone's build before they ask. (a) MVS, reopening decision 2
because its registry index is gone. (b) keep ranges and a solver.
**Recommendation: (a).**

```text
use dep.billing.{Invoice}   # billing 1.4.2 unless some manifest asks for more
```

**DP6. May a tagged release depend on an untagged commit?** Effect:
whether a library can ship an unreleased fix of its dependency. (a) yes,
through a pseudo-version, as Go allows. (b) no; releases need tagged
dependencies. **Recommendation: (a).** It is still a pinned commit with a
hash.

```text
use dep.pdf.{render}   # pdf = "github.com/acme/pdf@0.4.1-0.20260912081500-3f2c9e1a7b6d"
```

**DP7. Where does the compatibility check run without a registry?**
Effect: who catches a breaking change tagged as a minor. (a) `hd release`
before tagging, plus `hd get -u` before upgrading. (b) only at release.
(c) nowhere. **Recommendation: (a).**

```text
use dep.json.{Value}   # hd get -u: json 2.2.0 removes Value.size: incompatible-upgrade
```

**DP8. Which integrity services does `hd` rely on?** Effect: what a user
trusts on the first download of a version. (a) a committed `hd.sum` only.
(b) plus a public checksum log that `hd` checks by default. (c) plus a
caching proxy. **Recommendation: (a) now, with (b) and (c) as optional
settings once someone runs them.** Builds must work with direct fetches.

```text
use dep.json.{Value}   # hd.sum mismatch for json 2.1.0: checksum-mismatch
```

**DP9. How are private repositories fetched?** Effect: whether any
credential touches the repository or hd. (a) through `git` and its
credential helpers, plus a private-path pattern that skips any public log
or proxy. (b) an hd credential store. **Recommendation: (a).**

```text
use dep.billing.{Invoice}   # git.example.com is private: fetched with the user's git auth
```

**DP10. Are workspaces committed?** Effect: whether CI builds the same
graph as a developer. (a) a committed `[workspace]` with one selection, as
PACKAGES drafts. (b) a local-only file, as Go's `go.work` is.
**Recommendation: (a).** A monorepo's members are released together.

```text
use dep.shared.{Money}   # shared is a workspace member
```

**DP11. Is vendoring supported?** Effect: whether a build survives a
deleted repository without a proxy. (a) `hd vendor` writes `vendor/`, and
builds use it when present. (b) no vendoring. **Recommendation: (a).**
It also serves durable replay code identity offline.

```text
use dep.json.{Value}   # read from vendor/github.com/acme/json when present
```

**DP12. May one repository hold several packages?** Effect: whether a
folder extracted into a package ([DC C16](DEPENDENCY_CYCLES.md#c16-a-folder-becomes-a-package)) can stay
in its repository. (a) yes, a subdirectory path with prefixed tags such
as `lint/v1.2.0`, as Go allows. (b) one package per repository.
**Recommendation: (a).**

```text
use dep.lint.{check}   # lint = "github.com/acme/tools/lint@1.2.0"
```

**DP13. Which hosts can a path name?** Effect: whether `hd` must fetch
HTML to find a repository. (a) known hosts plus an explicit `.git`
qualifier for others. (b) also Go's `go-import` meta-tag discovery for
vanity paths. **Recommendation: (a)** at first; (b) can be added later
without changing existing paths.

```text
use dep.billing.{Invoice}   # billing = "git.example.com/shop/billing.git@0.4.2"
```

**DP14. Must a durable deployment reject a path `[patch]`?** Effect:
whether code identity can include a tree nobody else can fetch. (a) no;
the build hashes the patched tree into the identity. (b) yes; durable
builds need only fetched or vendored sources. **Recommendation: (a).**
The identity stays exact, and replay needs the same artifact anyway.

```text
use dep.json.{Value}   # json patched to ../json: identity uses that tree's hash
```

## Sources

1. Go Modules Reference: module paths, major version suffixes,
   pseudo-versions, MVS, `go.sum`, `GOPROXY`, `GOSUMDB`, `GOPRIVATE`,
   `replace`, `exclude`, `retract`, workspaces, vendoring, the `go` and
   `toolchain` lines. <https://go.dev/ref/mod>
2. Go 1.24 Release Notes: `GOAUTH` and `tool` directives.
   <https://go.dev/doc/go1.24>
3. Russ Cox, "Minimal Version Selection".
   <https://research.swtch.com/vgo-mvs>
4. Go checksum database design (proposal 25530).
   <https://go.googlesource.com/proposal/+/master/design/25530-sumdb.md>
5. Go Toolchains. <https://go.dev/doc/toolchain>
6. Deno, "What we got wrong about HTTP imports".
   <https://deno.com/blog/http-imports>; Deno modules:
   <https://docs.deno.com/runtime/fundamentals/modules/>
7. JSR, "Why JSR?". <https://jsr.io/docs/why>
8. SwiftPM, `package(url:_:)` and version requirements.
   <https://docs.swift.org/swiftpm/documentation/packagedescription/package/dependency/package(url:_:)-2ys47/>
9. SwiftPM PackageDescription reference.
   <https://docs.swift.org/package-manager/PackageDescription/PackageDescription.html>
10. Nix flakes: inputs, `flake.lock`, `follows`.
    <https://nix.dev/manual/nix/2.24/command-ref/new-cli/nix3-flake>
11. Zig, `build.zig.zon` documentation.
    <https://github.com/ziglang/zig/blob/master/doc/build.zig.zon.md>
12. Gleam, `gleam.toml` reference.
    <https://gleam.run/documentation/gleam-toml-reference>
13. Cargo, specifying dependencies from git repositories.
    <https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html>
14. Socket, "Go Supply Chain Attack: Malicious Package Exploits Go Module
    Proxy Caching for Persistence" (February 2025).
    <https://socket.dev/blog/malicious-package-exploits-go-module-proxy-caching-for-persistence>
15. Natalie Weizenbaum, "PubGrub: Next-Generation Version Solving".
    <https://nex3.medium.com/pubgrub-2fb6470504f>

## Parse Log

Every `text` block was parsed with `parseSource` from
[the reference parser](../spec/reference-parser/parser.ts). Parsing checks
syntax only; no block is claimed to type-check.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Option A source, with a `tests:` block | parses |
| 2 | Option B source | parses |
| 3 | Option C source | `syntax-error` at line 1, where the parser stops; every line is marked `# hypothetical syntax` |
| 4 | Option D source | parses |
| 5-18 | DP1 to DP14 examples | parse |

Two further lines were parsed outside this file, as evidence for the
Identity section: `use github.com/acme/json.Encoder` gives `syntax-error`,
and `use dep.github.com.acme.json.Encoder` parses.
