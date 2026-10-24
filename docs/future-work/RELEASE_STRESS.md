# Releasing A Package: Stress Test

Design review, 2026-10-24; changes no decision, specification, CLI or compiler behavior.
This tests [RELEASE.md](RELEASE.md), against the [owner's single-command direction](https://github.com/HerringtonDarkholme/hd-lang/issues/28#issuecomment-6377131231)
and [tool-comparison request](https://github.com/HerringtonDarkholme/hd-lang/issues/28#issuecomment-6377142148).
Nothing in this report accepts a compatibility policy or implements `hd release`.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [How Existing Tools Release](#how-existing-tools-release)
- [Problems, Ranked](#problems-ranked)
- [Parse Log And Validation](#parse-log-and-validation)
- [Owner Questions](#owner-questions)

## Surface Being Tested

The latest design has **one command**, with an explicit version selecting its writing mode.
Historical references to separate reader/writer names do not override that direction.
All three forms analyze committed HEAD and selected package history, rather than mutable source files.
No tool in this study is invoked to publish or push anything.

| Invocation | Proposed result | Repository mutation |
| --- | --- | --- |
| `hd release` | API findings and minimum suggestion; completed reports exit 0, including breaking findings | No source, manifest or tag writes; a dirty checkout is allowed and identified |
| `hd release VERSION --dry-run` | Every writer preflight, including adequate version, clean tree and absent target tag | No tag creation |
| `hd release VERSION` | Same validation, followed by guarded local annotated tag creation | One tag ref; no manifest version, commit, hook, push or publication |

Scratch checks and caches are reader effects, as [Platforms, authority and failure](RELEASE.md#platforms-authority-and-failure) already states.
“Creates nothing” is tested as no release artifact or repository mutation, rather than no ephemeral storage allocation.
The writer's failed transaction may leave an unreachable tag object, but must leave no partial release ref.
A dry run is evidence about its pinned inputs, not a reservation against later changes.

The proposed comparator projects checked interfaces, including resolved re-exports, generated impls, generics, rows and default-presence facts.
It normalizes source-order noise and binder names, then applies the design's conservative classification tables.
Unknown extraction coverage produces `unassessed`; body-level behavior remains outside the checked assessment.
No public cache schema or rendered-Markdown API comparison is assumed.

Relevant accepted rules are [Git versions and compatibility lines](../../spec/lang/10-modules.md#versions),
[re-export identity](../../spec/lang/10-modules.md#r-module.pub-use.identity),
[generic bound inference](../../spec/lang/04-type-system.md#r-types.generic.infer.bound),
[function references](../../spec/lang/07-functions.md#r-fn.ref.value),
[generated derivation impls](../../spec/lang/09-traits.md#r-trait.derive.generate),
and [row subsumption](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume).
The release command itself remains [future CLI tooling](../../spec/cli/command-line.md#r-cli.tooling.package-later).
Std collection operations in the witnesses follow [List](../../spec/std/collections.md#list-methods).

## Method

Nineteen cases combine three real library histories with hd-specific type hazards and Git/version operations.
Rand contributes two distinct edits from its 0.8-to-0.9 transition; logr and Lodash supply the other histories.
Each translation isolates the stated edit and deliberately simplifies unrelated behavior.
No isolated witness is presented as a complete foreign release audit.

The first fourteen cases contain old/new library packages and one unchanged client.
They use the same `api` manifest name and a scratch client dependency `api = { path = "../api" }`.
That path dependency is a testing convenience, not a releasable manifest.
A tagged release must follow the existing [versioned path-dependency rule](../../spec/lang/10-modules.md#r-module.version.no-bare-path-release).

*Works* means the proposed design expresses the required assessment or guard.
*Friction* means conservative classification or an acknowledged scope limit needs human review.
*Breaks* means the written design cannot give one consistent answer for the case.
A rejected new client is successful evidence for detecting an API break, not a failed stress test.

Every hd block is parsed; complete packages are additionally checked.
Passing clients run on Node and Wasmtime, with per-version output compared between engines.
The future comparator and release CLI do not exist, so their proposed findings are evaluated against the design tables.
The arithmetic and Git probes are separate executable models, never claimed as end-to-end CLI tests.

One source activity is a per-file format/parse, a package check, or one engine run.
Intentional downstream failures count only when the recorded outcome matches the expected rejection.
The final evidence has 58 source files, 56 package checks and 36 engine runs, totaling 208 expected-outcome activities.
Repeated source fragments and ordinary formatting passes are not counted as additional independent API cases.

## Summary

| Case | History or operation | Verdict | Proposed outcome / exposed limitation |
| --- | --- | --- | --- |
| C1 | Rand retained rename aliases | works | Compatible addition when the old path and contract remain |
| C2 | Rand Uniform construction becomes fallible | works | Definite break; return type changes |
| C3 | logr Logger interface becomes a struct | works | Definite break for downstream implementers |
| C4 | Lodash removes pluck | works | Definite removed-public-path break |
| C5 | Re-export identity and an added alias | works | Preserve old identities; added path alone is compatible |
| C6 | Added generic trait impl | friction | Real inference break justifies potential-major conservatism |
| C7 | Removed applicable impl | works | Definite obligation failure |
| C8 | Added requirement | works | Definite caller failure |
| C9 | Removed requirement inside an inferred container | works | Definite invariant nested-type failure |
| C10 | Tightened generic bound | works | Definite instantiation failure |
| C11 | Added defaulted value parameter | works | Stored function arity breaks despite surviving direct calls |
| C12 | Added default to an existing parameter | works | Compatible new call form; stored arity unchanged |
| C13 | Removed Eq derive | works | Generated-impl loss breaks an operator client |
| C14 | Changed default value, unchanged type | friction | Typed API is patch; observed behavior changes |
| C15 | Prereleases, minimum versions and 0.x | works | Every version is compared with the last stable release |
| C16 | Workspace member, directory prefix and ancestry | works | Ignore unrelated prefixes and disconnected higher tags |
| C17 | First release versus a bad baseline | works | No default baseline differs from an invalid explicit baseline |
| C18 | Dirty reader, writer and dry run | works | Read pinned blobs; writer modes refuse dirty states |
| C19 | Existing tags and concurrent ref changes | works | Guard target absence and exact pinned ref objects atomically |

Sixteen cases work, two expose friction, and one exposes inconsistent arithmetic rules.
The proposal handles the typed API adversaries substantially better than a function-signature or rendered-document diff.
The current release arithmetic model needs correction before it can support implementation.
The report leaves that policy choice with the owner.

## Cases

### C1. Rand keeps the renamed entry point

Rand's [0.9 migration guide](https://rust-random.github.io/book/update-0.9.html) says `thread_rng` becomes `rng`, while the old name remains deprecated.
This is not removal of the old function.
The isolated source shape is:

```rust
let first = rand::thread_rng(); // retained compatibility entry
let second = rand::rng();      // newly preferred entry
```

The hd witness returns a fixed marker instead of a random generator.
It tests name preservation and equivalent calls, not randomness, deprecation diagnostics or generator behavior.

Old library:

```text
pub fn thread_rng() -> usize: 7
```

New library:

```text
pub fn rng() -> usize: 7
pub fn thread_rng() -> usize: rng()
```

Unchanged client:

```text
use dep.api.thread_rng
pub fn main() -> void:
    if thread_rng() != 7: panic("old name disappeared")
```

Both old and new clients check and run on both engines.
The comparator should pair the retained `thread_rng` contract and report only the new `rng` path as an addition.
This isolated delta is `minor`; other changes in Rand 0.9 are not thereby certified compatible.
**Verdict: works.**

### C2. Rand makes Uniform construction fallible

The same [migration guide](https://rust-random.github.io/book/update-0.9.html) records a return-type change for `Uniform::new` and `new_inclusive`.
The new constructors return a `Result` instead of panicking on invalid inputs.
The adapted original call changes from direct construction to explicit error handling:

```rust
let interval = Uniform::new(low, high)?;
```

The hd witness uses a free function and one public field to expose the same return-contract change.
Its error type is deliberately minimal.

Old library:

```text
pub struct Uniform:
    pub upper: usize
pub fn uniform(low: usize, high: usize) -> Uniform:
    Uniform { upper: high }
```

New library:

```text
pub struct Uniform:
    pub upper: usize
pub struct RangeError: pass
pub fn uniform(low: usize, high: usize) -> Result[Uniform, RangeError]:
    if low >= high: return .Err(RangeError {})
    .Ok(Uniform { upper: high })
```

Unchanged client:

```text
use dep.api.uniform
pub fn main() -> void:
    if uniform(1, 9).upper != 9: panic("upper")
```

Both library versions check; the unchanged new client rejects with `unknown-field` on `Result[Uniform, RangeError]`.
The comparator must compare the result contract, not infer compatibility because valid calls still describe the same interval.
A code generator produces a result tag and explicit error branch in the new implementation.
**Verdict: works; definite major API magnitude**, which uses hd's 0.x line arithmetic when applicable.

### C3. logr changes Logger from an interface to data

In [logr v0.4.0](https://github.com/go-logr/logr/blob/v0.4.0/logr.go), `Logger` is an interface.
In [v1.0.0](https://github.com/go-logr/logr/blob/v1.0.0/logr.go), it is a struct delegating work to a sink.
The original shape change is:

```go
// old
var log Logger = customLogger{}
// new
var log Logger = New(customSink{})
```

The hd translation isolates a downstream implementation of the former interface.
It models an `enabled` query, not the full structured-logging API.

Old library:

```text
pub trait Logger:
    fn enabled(self) -> bool
```

New library:

```text
pub struct Logger:
    pub active: bool
impl Logger:
    pub fn enabled(self) -> bool: self.active
```

Unchanged client:

```text
use dep.api.Logger
struct Sink: pass
impl Logger for Sink:
    fn enabled(self) -> bool: true
pub fn main() -> void:
    let logger: &dyn Logger = &Sink {}
    if !logger.enabled(): panic("disabled")
```

The old client runs; the new client rejects twice with `unknown-trait` because `Logger` is no longer a trait.
Comparing only same-named callable members would miss this client category.
The snapshot's declaration-kind and downstream-implementation contracts detect the break before method-body comparison.
**Verdict: works; definite major.**

### C4. Lodash removes pluck

[Lodash's 4.0 changelog](https://github.com/lodash/lodash/wiki/Changelog) removes `pluck` in favor of `map` with property shorthand.
A replacement operation does not preserve the old exported name:

```javascript
const oldNames = _.pluck(records, "name");
const newNames = _.map(records, "name");
```

hd has no JavaScript property shorthand in this witness.
A typed record's `value` is the fixed projection, and the replacement function is named `map_values`.
This preserves the removed-name and equivalent-result issue without pretending to port Lodash's dynamic collection semantics.

Old library:

```text
pub struct Record:
    pub value: usize
pub fn pluck(values: List[Record]) -> List[usize]:
    let mut out: List[usize] = []
    for item in values:
        out.push(item.value)
    out
```

New library:

```text
pub struct Record:
    pub value: usize
pub fn map_values(values: List[Record]) -> List[usize]:
    let mut out: List[usize] = []
    for item in values:
        out.push(item.value)
    out
```

Unchanged client:

```text
use dep.api.{Record, pluck}
pub fn main() -> void:
    if pluck([Record { value: 7 }])[0] != 7: panic("missing projection")
```

The new client rejects with `unknown-import` for `pluck`; its later operand diagnostic is secondary.
The comparator must retain the removed public path finding even when an author documents a straightforward migration.
The old implementation's result also runs on both engines.
**Verdict: works; definite major.**

### C5. Re-export identity is not the lexically first alias

This operation ports the identity problem typed API comparators must solve across releases.
Both public paths denote one existing nominal declaration.
The new alias sorts before `Token`, but must not rename that declaration in the snapshot.

Old library:

```text
pub use pkg.model.{Token}
```

New library:

```text
pub use pkg.model.{Token}
pub type Alias = Token
```

Shared `model.hd`, unchanged in both library versions:

```text
pub struct Token:
    pub value: usize
```

Unchanged client:

```text
use dep.api.Token
use dep.api.model.{Token as Original}
fn pass_through(value: Original) -> Token: value
pub fn main() -> void:
    if pass_through(Original { value: 7 }).value != 7: panic("identity changed")
```

Both clients assign the original module's `Token` to the facade's `Token` and run successfully.
A canonical identity chosen from the first alias would create a false type change when `Alias` appears.
The design's correspondence by shared old paths and identity partitions avoids that error.
**Verdict: works; compatible added alias path.**

### C6. An added impl changes generic inference

This is the existing hd `Source[U]` inference witness, rechecked on the current compiler.
The library keeps every original declaration and adds one implementation.

Old library:

```text
pub trait Source[T]:
    fn read(&self) -> T

pub struct Feed: pass

impl Source[i32] for Feed:
    fn read(&self) -> i32: +7

pub fn read_source[U, S < Source[U]](source: S) -> U:
    source.read()
```

New library:

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

Unchanged client:

```text
use dep.api.{Feed, read_source}

pub fn main() -> void $ Console:
    value := read_source(Feed {})
    println(value)
```

The old client prints 7; the new client rejects with `ambiguous-type`, followed by `cannot-infer-type`.
Adding an applicable impl is therefore not automatically compatible.
The proposed potential-major rule correctly covers this case, but also conservatively covers additions whose clients happen to remain valid.
**Verdict: friction; potential-major conservatism has a real witness and a deliberate false-positive cost.**

### C7. Removing an impl preserves signatures but loses an obligation

This witness removes a written implementation while retaining the public function and data type.

Old library:

```text
pub trait Mark:
    fn code(self) -> usize
pub struct Token: pass
impl Mark for Token:
    fn code(self) -> usize: 7
pub fn read[T < Mark](value: T) -> usize: value.code()
```

New library:

```text
pub trait Mark:
    fn code(self) -> usize
pub struct Token: pass
pub fn read[T < Mark](value: T) -> usize: value.code()
```

Unchanged client:

```text
use dep.api.{Token, read}
pub fn main() -> void:
    if read(Token {}) != 7: panic("marker")
```

The new library checks, but its client reports `unsatisfied-trait-bound: Token does not implement Mark`.
The comparator needs observable impl applicability, not only exported declaration signatures.
No declaration-level `pub` on the implementation is needed for this break.
**Verdict: works; definite major.**

### C8. An added requirement breaks a caller

A function with unchanged value parameters and result gains a requirement.
The witness preserves its return value.

Old library:

```text
pub fn answer() -> i32: +42
```

New library:

```text
use std.time.Clock

pub fn answer() -> i32 $ Clock: +42
```

Unchanged client:

```text
use dep.api.answer

pub fn main() -> void:
    if answer() != +42:
        panic("wrong answer")
```

The new client reports `missing-requirement` because its row does not provide Clock.
The body returning the same integer does not make the interface compatible.
The snapshot must normalize and compare the function's row.
**Verdict: works; definite major.**

### C9. Removing a requirement can break a nested inferred type

The converse edit survives some direct calls but changes the type inferred inside a generic container.

Old library:

```text
use std.time.Clock

pub fn answer() -> i32 $ Clock: +42
```

New library:

```text
pub fn answer() -> i32: +42
```

Unchanged client:

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

The old client runs; the new client reports `type-mismatch` between `Holder[fn() -> i32 $ Clock]` and `Holder[fn() -> i32]`.
Row adaptation at a direct argument does not rewrite an invariant nested type.
The design's treatment of row changes in both directions catches this client.
**Verdict: works; definite major for the witness.**

### C10. A new generic bound excludes an old instantiation

The body remains the identity function while its generic contract changes.

Old library:

```text
pub fn identity[T](value: T) -> T: value
```

New library:

```text
pub fn identity[T < Display](value: T) -> T: value
```

Unchanged client:

```text
use dep.api.identity
struct Item:
    value: usize
pub fn main() -> void:
    if identity(Item { value: 7 }).value != 7: panic("identity")
```

The new client rejects with `unsatisfied-trait-bound: Item does not implement Display`.
The comparison must retain the normalized bound list even though it is unnecessary to the function's body.
Renaming a binder alone would instead normalize away; tightening its obligations must not.
**Verdict: works; definite major.**

### C11. A defaulted parameter changes a stored function value

The ordinary one-argument call remains available, but an old stored function has the original arity.

Old library:

```text
pub fn shift(value: i32) -> i32: value
```

New library:

```text
pub fn shift(value: i32, extra: i32 = +0) -> i32: value + extra
```

Unchanged client:

```text
use dep.api.shift

pub fn main() -> void:
    let callback: fn(i32) -> i32 = shift
    if callback(+3) != +3:
        panic("wrong callback")
```

The new client rejects with `type-mismatch` between one- and two-argument function types.
A tool that tests only direct calls would misclassify the edit as compatible.
The design's explicit callable/default distinction handles it.
**Verdict: works; definite major.**

### C12. A default on an existing parameter adds a call form

This edit keeps both parameter positions and types.
It changes only whether callers may omit the second argument.

Old library:

```text
pub fn shift(value: i32, extra: i32) -> i32: value + extra
```

New library:

```text
pub fn shift(value: i32, extra: i32 = +0) -> i32: value + extra
```

Unchanged client:

```text
use dep.api.shift

pub fn main() -> void:
    let callback: fn(i32, i32) -> i32 = shift
    if callback(+3, +1) != +4:
        panic("wrong callback")
```

Old and new clients keep the same stored function type and run on both engines.
The new declaration additionally supports omission of the defaulted argument.
Comparing only presence-bit inequality as a break would be too conservative here.
**Verdict: works; minor API addition.**

### C13. Removing a derive removes an ordinary impl

The source edit deletes one decorator rather than a function.
Its effect on clients is an operator-contract removal.

Old library:

```text
@derive(Eq)
pub struct Point:
    pub x: i32
```

New library:

```text
pub struct Point:
    pub x: i32
```

Unchanged client:

```text
use dep.api.Point

pub fn main() -> void:
    if Point { x: +1 } != Point { x: +1 }:
        panic("unequal")
```

The new client reports `type-mismatch: Point does not implement Eq for this operand`.
The comparator must inspect generated impls after derivation, including their bounds.
Replacing a derive with an equivalent written implementation should instead preserve this contract.
**Verdict: works; definite major.**

### C14. A changed default value is a behavioral change

The default remains present, the arity is unchanged and its expression keeps the same type.

Old library:

```text
pub fn count(value: usize = 1) -> usize: value
```

New library:

```text
pub fn count(value: usize = 2) -> usize: value
```

Unchanged client:

```text
use dep.api.count
pub fn main() -> void $ Console:
    println(count())
```

Both versions check and run on both engines; the old client prints 1 and the new client prints 2.
The design correctly assigns no typed API break, but that is not proof of a patch release's behavioral compatibility.
Its source fingerprint should produce a review note, with `behavior_assessed: false` still visible.
**Verdict: friction; signatures alone cannot set the author's complete SemVer obligation.**

### C15. Prerelease rules and the suggested minimum

This case reuses C1's compatible addition and C2's breaking return change under different tag histories.
[SemVer precedence](https://semver.org/spec/v2.0.0.html) orders a prerelease below the corresponding final version.
The [arithmetic](RELEASE.md#bump-arithmetic) compares every version, prerelease or final, with the last stable release.
With stable `1.4.2`, the versions `1.5.0-rc.1`, `1.5.0-rc.2` and `1.5.0` are each compared with `1.4.2`.
Prereleases of one core may add or change API freely among themselves.

The suggestion names the smallest stable core.
After stable `1.4.2`, a break suggests `2.0.0`, and an author-written `2.0.0-rc.1` also passes, though it sorts below `2.0.0`.

Major-zero arithmetic is separately consistent with hd's [compatibility lines](../../spec/lang/10-modules.md#r-module.version.line).
A break at `0.4.2` requires `0.5.0`; an addition can use `0.4.3`.
The line of `0.0.2` is `0.0`, so the proposal also requires `0.1.0` for a break there.
That differs from tools that deliberately relax all pre-1.0 breaking-change checks.

The 33 recorded arithmetic observations were taken before the stable-baseline rule; the 24 agreed version decisions and six malformed-version rejections still hold.
They include numeric `rc.9`/`rc.10` ordering, numeric minor ordering and rejected metadata.
These are models of the prose, not a release implementation.
**Verdict: works.**

### C16. A workspace member uses its directory, not its manifest name

This case gives C5's library different package metadata and surrounding tags.
The selected package lives at `tools/lint`, with manifest name `linter`.
The design must use the directory prefix without accidentally selecting another member's history.

| Local tag | Graph relationship | Eligible baseline? |
| --- | --- | --- |
| `tools/lint/v1.9.0` | ancestor | yes |
| `tools/lint/v1.10.0` | ancestor | yes, selected numerically |
| `tools/lint/v9.0.0` | disconnected history | no |
| `other/v20.0.0` | ancestor, another package prefix | no |

A disposable Git repository confirms the candidate set and ancestry checks; the selection model chooses `tools/lint/v1.10.0`.
The writer would create `tools/lint/vVERSION`, not `linter/vVERSION`.
This does not implement workspace package discovery, coordinated version bumps or `-p` parsing.
**Verdict: works; the future CLI must test those integration points separately.**

### C17. First release is not a fallback after a failed check

This case uses an unchanged tiny library and three baseline states.
The disposable repository's committed API is:

```text
pub fn answer() -> usize: 1
```

| Baseline state | Proposed assessment | Writer condition |
| --- | --- | --- |
| No matching default tag | `first-release` | Current library must still check; author chooses a valid exact version |
| Explicit baseline missing locally | failure / `unassessed` | No tag |
| Matching tag names a blob, not a commit | failure / `unassessed` | No tag |

The Git probe confirms no tags, missing-ref failure and commit-peeling failure for a blob tag.
The protocol model preserves the three distinct outcomes.
Missing dependencies or an unsupported snapshot must likewise fail, rather than creating an accidental first release.
**Verdict: works; no compatibility conclusion is available for an actual first release.**

### C18. Dirty readers use committed data; writers refuse

The changed-default shape of C14 supplies the semantic contrast: a committed function returns 1, while an edit returns 2.
A Git-object reader still retrieves the committed bytes after a working-file edit.
The read-only report identifies dirty state but does not silently analyze the changed function.

The disposable probe checks unstaged changes, staged changes and nonignored untracked files separately.
All make the writer and its dry-run ineligible under the proposed clean-tree guard.
The dry-run model checks the clean tree and pinned HEAD without entering tag-object/ref creation, then confirms the ref inventory is unchanged.
This is a protocol exercise, not proof that a future `hd release` already enforces it.

A successful dry run cannot promise a later write will succeed.
The later invocation must repeat validation against its own pinned inputs and transaction guards.
No application main, tests or release hooks should run during the proposed API extraction.
**Verdict: works; reader/writer input provenance must remain explicit in the report.**

### C19. An existing tag and a racing ref are not overwrite permission

The unchanged API from C17 is checked conceptually against a pinned baseline and HEAD.
The implementation primitive under test is a real local ref transaction:

```sh
start
verify HEAD <checked-head>
verify refs/tags/v1.0.0 <pinned-tag-object>
create refs/tags/v1.0.1 <new-annotated-tag-object>
prepare
commit
```

The tokens above are a protocol transcript for `git update-ref --stdin`, not standalone shell commands.
See [Git's transaction contract](https://git-scm.com/docs/git-update-ref).
The fresh repositories use explicit empty hooks and disabled fsmonitor configuration.

| Race or state | Actual plumbing outcome |
| --- | --- |
| Stable HEAD, stable baseline, absent target | One annotated target tag created |
| HEAD moved to another commit | Transaction fails; target absent |
| Baseline moved to another commit | Transaction fails; target absent |
| Annotated baseline replaced, same peeled commit | Raw object guard fails; target absent |
| Target already exists at another commit | Transaction fails; old target unchanged |
| Target already exists at the checked commit | Transaction fails; old target unchanged |
| Symbolic HEAD switches branches at the same commit | Transaction succeeds; checked committed input is unchanged |
| Detached HEAD at the checked commit | Transaction succeeds |

The same-commit target case is intentional: an existing version is not a request to retag it.
The same-commit baseline replacement case proves that guarding only the peeled commit would be insufficient.
These are local macOS Git-plumbing results, not Windows CLI validation or a durable filesystem crash test.
**Verdict: works; all final ref checks must share one atomic transaction.**

## How Existing Tools Release

This is a source review of the linked documentation, retrieved 2026-10-24, not execution of foreign release commands.
“Dry run” below names the documented command behavior, not a guarantee that caches, packing or lifecycle scripts have no local effects.
Registry distribution tags and Git tags are different objects.
None of these tools makes a universal proof of behavioral compatibility.

| Tool / command | Bare invocation | What writes | Version choice | Dry run | Tags | Checks |
| --- | --- | --- | --- | --- | --- | --- |
| `cargo publish` | Packages and uploads the selected manifest version | Package artifacts and registry upload | Existing Cargo manifest version | `--dry-run` checks/packages without upload | No automatic Git tag | Packaging, metadata and build verification unless disabled; no general API-diff gate |
| `cargo release` | Previews release steps by default | With `--execute`: configured version files, commits, publication and remote effects | Exact version or bump level; default `release` removes a prerelease suffix | Default mode; execution is explicit | Configurable Git tags and pushes | Release/configuration checks; API compatibility is a separate concern |
| `cargo semver-checks` | Compares the current crate against a registry baseline | Analysis/cache artifacts, not a release | Checks compatibility against an explicit or inferred release type; does not choose/publish a release artifact | Already an analysis command | Reads a baseline revision/version; creates no release tag | Typed rustdoc JSON compatibility rules, selected features and target coverage |
| `gorelease` | Compares main-module API/dependencies and suggests a version | Analysis scratch/cache state | Suggests without `-version`; checks an explicit `-version` | Already report-only | No tag creation | Go API, dependencies and module-version constraints |
| `git tag` | Lists local tags | A named invocation creates a local tag ref | Exact author-written name; no SemVer inference | No general creation `--dry-run`; listing is read-only | Lightweight by default, annotated with `-a`/message or signing options | Ref/object/name validity; no API comparison |
| Go module proxy workflow | No `publish` subcommand; consumers request versions | Author pushes Git tags; proxy retrieves and caches module content | Repository tags and Go module-path/version rules | No unified release dry run | Git version tags belong to the repository | Module/version/content validation, not API-diff certification |
| `pnpm publish` | Uploads the manifest version | Packing/lifecycle effects and registry publication | Existing package version | `--dry-run` omits the upload | Registry dist-tag such as `latest`; does not itself version/tag the Git tree | Publish/Git guards and package rules; not typed API comparison |
| `npm version` | Lists package/runtime versions | With an argument: version files and normally a Git commit/tag, plus lifecycle scripts | Exact version, bump keyword or Git-derived version | No equivalent documented preview for the full version mutation | Defaults to a Git version tag/commit when eligible | Version validity and clean-tree/script workflow; no API inference |
| `changeset`, `version`, `publish`, `status` | Bare `changeset` interactively records change intent | Add writes a changeset; version updates versions/dependencies/changelogs; publish uploads | Author-selected change intent aggregated across packages | `status` previews intent; no general `--dry-run` promises across all subcommands | Publish/git-tag create local Git tags; pushing is separate | Declared release intent and package coordination; not signature-derived SemVer |

### Cargo publish

[Cargo publish](https://doc.rust-lang.org/cargo/commands/cargo-publish.html) supplies a useful distinction between preparing a package and actually uploading it.
hd should take the explicit no-upload preview idea, while keeping its local tag operation visibly narrower.
Cargo's packaged-source build check is stronger than mere archive construction, but does not replace hd's typed API comparison.
The initial hd command should not silently inherit registry publication or Cargo's configurable verification bypass.

### cargo-release

[cargo-release's reference](https://github.com/crate-ci/cargo-release/blob/main/docs/reference.md) makes preview the default and execution explicit.
Its default bump is `release`, not automatic inference from public types.
hd should take explicit mutation and independent steps, while avoiding hooks, implicit pushes and broad orchestration in this feature.
The owner's one-command surface remains the baseline; this comparison does not reopen command naming.

### cargo-semver-checks

[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) compares structured rustdoc data and accepts registry, Git, source-root or JSON baselines.
Its feature-selection policy illustrates why configuration coverage must be part of provenance.
hd should retain typed findings and explain unsupported coverage rather than equating one configuration with all possible clients.
The three translated histories were not run through this Rust tool.

### gorelease

[gorelease](https://pkg.go.dev/golang.org/x/exp/cmd/gorelease) closely matches the proposal's report-and-check workflow without performing the release.
Its pre-1.0 behavior is deliberately looser: incompatible API changes do not affect the exit status for a major-zero target.
hd should take explicit baseline/version reporting, while preserving its own compatibility-line policy rather than importing that exception.
The logr translation is an hd type witness, not a claim to have run gorelease on logr.

### Git tags

[git tag](https://git-scm.com/docs/git-tag) separates listing from explicit ref creation but does not decide release adequacy.
hd should use explicit author-selected versions and atomic ref guards.
It should avoid overwriting an existing version, including a tag at the same commit.
The local transaction probes exercise those primitives without configuring any repository remote.

### Go publication and the module proxy

[Go's module publication model](https://go.dev/ref/mod#publishing) makes repository tags discoverable through version/content retrieval.
The proxy is not a compatibility approval service.
hd should preserve exact repository/package/version identity, while avoiding a network operation hidden inside its report command.
A successful local tag is not evidence that a remote consumer can already fetch it.

### pnpm publish

[pnpm publish](https://pnpm.io/cli/publish) publishes existing package versions and distinguishes registry dist-tags from version numbers.
hd should take explicit package selection and honest dry-run scope.
It should avoid inheriting publication/lifecycle side effects, or confusing a successful upload with an immutable local release transaction.
The online documentation spans pnpm versions; this review does not assert every current option exists in the repo's pnpm 10.8 installation.

### npm version

The [npm version documentation](https://docs.npmjs.com/cli/v11/commands/npm-version/) describes argument-driven version mutation and lifecycle behavior.
Its [command source](https://github.com/npm/cli/blob/latest/lib/commands/version.js) confirms that no-argument invocation lists versions.
hd should take exact-version mutation as an explicit choice, while avoiding automatic source commits and scripts.
A release preview should not be described as an npm-version dry run when that is not its documented operation.

### Changesets

[Changesets' commands](https://github.com/changesets/changesets/blob/main/docs/command-line-options.md) record human release intent, update package versions, then publish separately.
`status` reports that recorded plan; it does not infer API compatibility.
hd should take the clear reportable package plan and keep publication separate.
Coordinated workspace dependency rewriting is a larger feature and remains outside the proposed single-package tag writer.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | REL3 | Typed compatibility does not certify behavior or every configured build | Silent limit unless prominently reported | C1, C2, C14; all assessments |
| 2 | REL4 | Conservative impl classification trades false positives for soundness | Review and release churn | C6, C7, C10, C13 |
| 3 | REL5 | Passing component probes do not establish an end-to-end release transaction | Implementation verification gap | C16–C19 |

### REL3. Typed findings versus release compatibility

**Effect.** C14 changes observable output without changing a typed contract.
Rand's real history also changes generator behavior beyond the isolated type witnesses.
The design's `behavior_assessed: false` is necessary and must not disappear from a successful writer summary.

**Candidates.** Should the success text say “typed API checks passed; behavior not assessed” rather than “compatible release”?
Should changed default/body fingerprints list specific review notes without trying to assign their behavioral magnitude?
Unassessed extraction coverage must remain a failure, unlike this deliberately unassessed behavior category.

### REL4. Conservative impl changes

**Effect.** C6 proves an added impl can break an unchanged client, while C7 and C13 prove removed impls affect usable contracts.
Testing only a package's known clients cannot certify every possible downstream client.
The initial rule may therefore require larger bumps for some changes a maintainer considers harmless.

**Candidates.** Should the first report retain distinct definite/potential findings and show a concrete witness when one is available?
Should any future waiver mechanism wait for real false-positive reports rather than enter this initial design?
This report does not propose weakening the owner-reviewed classification tables.

### REL5. Integration proof still has to be built

**Effect.** C16–C19 pass model/component checks, not an unavailable CLI.
The compiler projection, package discovery, clean-state handling and Git transaction must operate on the same pinned inputs.
A future implementation could pass the primitive tests while wiring those inputs incorrectly.

**Candidates.** Should each implementation slice require real CLI fixtures for bare, dry-run and write modes before the writer is enabled?
Should Windows coverage and injected source/ref changes be required before claiming platform-independent safety?
The existing local Git probes can seed those tests, but cannot replace them.

## Parse Log And Validation

| Block | Source | Parser |
| --- | --- | --- |
| 1 | `rand-alias-retained:old-api` | clean |
| 2 | `rand-alias-retained:new-api` | clean |
| 3 | `rand-alias-retained:client` | clean |
| 4 | `rand-fallible-uniform:old-api` | clean |
| 5 | `rand-fallible-uniform:new-api` | clean |
| 6 | `rand-fallible-uniform:client` | clean |
| 7 | `logr-trait-to-data:old-api` | clean |
| 8 | `logr-trait-to-data:new-api` | clean |
| 9 | `logr-trait-to-data:client` | clean |
| 10 | `lodash-pluck-removed:old-api` | clean |
| 11 | `lodash-pluck-removed:new-api` | clean |
| 12 | `lodash-pluck-removed:client` | clean |
| 13 | `reexport-identity:old-api` | clean |
| 14 | `reexport-identity:new-api` | clean |
| 15 | `reexport-identity:shared-model` | clean |
| 16 | `reexport-identity:client` | clean |
| 17 | `impl-added:old-api` | clean |
| 18 | `impl-added:new-api` | clean |
| 19 | `impl-added:client` | clean |
| 20 | `impl-removed:old-api` | clean |
| 21 | `impl-removed:new-api` | clean |
| 22 | `impl-removed:client` | clean |
| 23 | `rows-added:old-api` | clean |
| 24 | `rows-added:new-api` | clean |
| 25 | `rows-added:client` | clean |
| 26 | `row-shrunk:old-api` | clean |
| 27 | `row-shrunk:new-api` | clean |
| 28 | `row-shrunk:client` | clean |
| 29 | `generic-bound-added:old-api` | clean |
| 30 | `generic-bound-added:new-api` | clean |
| 31 | `generic-bound-added:client` | clean |
| 32 | `defaulted-parameter:old-api` | clean |
| 33 | `defaulted-parameter:new-api` | clean |
| 34 | `defaulted-parameter:client` | clean |
| 35 | `default-presence:old-api` | clean |
| 36 | `default-presence:new-api` | clean |
| 37 | `default-presence:client` | clean |
| 38 | `derive-removed:old-api` | clean |
| 39 | `derive-removed:new-api` | clean |
| 40 | `derive-removed:client` | clean |
| 41 | `default-body-changed:old-api` | clean |
| 42 | `default-body-changed:new-api` | clean |
| 43 | `default-body-changed:client` | clean |
| 44 | `git-model:committed-api` | clean |

All **44 embedded hd blocks parse**; their complete old/new packages supply imports and cross-file declarations.
The fourteen pairs contain **58 source files**, including the unchanged shared re-export declaration.
All 28 library checks and 14 old-client checks pass; four new clients pass and ten reject for the intended reason.
All successful check summaries contain zero warnings.

The 36 successful Node/Wasmtime executions agree per source version.
C14 deliberately differs across versions: 1 becomes 2 while each engine pair agrees.
Formatting, parsing, package checking and execution account for **208 expected-outcome source activities**.

The separate models record **33 arithmetic observations** and **12 disposable-Git scenarios**.
The arithmetic observations predate the stable-baseline rule for prereleases.
Git scenarios cover eight ref-transaction states plus prefix/ancestry, baseline validity, dirty states and a read-only dry-run model.
These models do not prove a future release CLI, snapshot extractor, Windows integration or physical crash behavior.

The final report and its two index/link targets have **104 valid local links**; existing RELEASE.md headings remain unchanged.
No specification IDs are added or retired, and no conformance fixture or exception row changes.
No full test suite ran.

Current-source CLI provenance is pinned to `59682131d2cb7da3fb916eda01f9b72a9454797b`, built in the dev profile.
The parser source is unchanged from its recorded frozen build; the CLI was rebuilt because current main had compiler changes.

## Owner Questions

1. **Passing type checks does not certify changed output.** Should every success summary retain explicit behavior/configuration limits and specific default/body review notes?
   C14's identical signatures print 1 and 2.
2. **Potential findings can cause release churn.** Should the initial implementation keep the conservative rules and defer any waiver mechanism until real users report false positives?
   C6 adds an impl and makes an old inference site ambiguous.
3. **The component probes are not an implemented command.** Should enabling the tag writer require end-to-end fixtures across all three modes and platforms?
   C19's same-commit retagging cases distinguish exact ref-object guards from weaker commit-only checks.
