# hd lint

Design proposal, 2026-10-23; this note changes no accepted behavior or
specification. It recommends a first implementation for [#651](https://github.com/HerringtonDarkholme/hd-lang/issues/651),
under the owner's [design-note order](https://github.com/HerringtonDarkholme/hd-lang/issues/58#issuecomment-6367238331).
The owner approves the proposal before implementation. The first lint incorporates
the [2026-10-23 clarification](https://github.com/HerringtonDarkholme/hd-lang/issues/651#issuecomment-6369394128).

## Contents

- [Purpose and current behavior](#purpose-and-current-behavior)
- [Other tools](#other-tools)
- [Implementation choices](#implementation-choices)
- [Recommendation](#recommendation)
- [Command and configuration](#command-and-configuration)
- [First lint](#first-lint)
- [Output and status](#output-and-status)
- [Implementation order and validation](#implementation-order-and-validation)
- [Evidence and parse log](#evidence-and-parse-log)

## Purpose and current behavior

An agent should receive actionable advice without turning style policy into
a language error. A package author should enable stricter CI through the
manifest. Existing `check`, `build`, and `fix` behavior should remain stable.

The owner placed likely-bug and style warnings in a future `hd lint`.
The first lint concerns writes to temporary receivers, allowed by
[`fn.method.auto-address.temporary`](../../spec/lang/07-functions.md#r-fn.method.auto-address.temporary).
That rule creates a new object and keeps it alive until the call returns.

Today [`hd check`](../../spec/cli/command-line.md#building-and-checking) performs checking,
with shared [JSON diagnostics](../../spec/cli/command-line.md#machine-output).
There is no lint command or lint configuration.
The CLI already shares diagnostic rendering with the driver and playground.

The fixed use cases are an agent reading JSON, a package enforcing CI
policy, and a developer intentionally mutating a reference-bearing temporary.
The last case must remain quiet without inspecting method bodies.

## Other tools

| Tool | Relevant existing approach | Lesson for hd |
| --- | --- | --- |
| [Clippy](https://doc.rust-lang.org/stable/clippy/usage.html) | A separate command supports allow, warn, and deny levels. | Separate invocation can share compiler facts. |
| [Go vet](https://pkg.go.dev/cmd/vet) | Reports suspicious constructs; findings or invalid invocation produce a nonzero status. | A finding need not imply an ill-typed program. |
| [ESLint](https://eslint.org/docs/latest/use/configure/rules) | Rules have off, warning, and error severities. Errors can enforce CI policy. | Existing severity fields can express enforcement. |
| [Ruff](https://docs.astral.sh/ruff/linter/) | Rule selection and suppression are configurable; unsafe fixes require explicit opt-in. | Advice does not justify an automatic edit. |
| [SwiftLint](https://github.com/realm/SwiftLint#configuration) | YAML lists disabled, opt-in, or exclusively enabled rules. | Avoid several overlapping selector mechanisms initially. |
| [detekt](https://detekt.dev/docs/introduction/configurations/) | Configuration controls rule activation and validates known configuration. | Reject misspelled rule names rather than silently ignoring them. |

These tools show that language acceptance and lint enforcement can remain
separate. hd can reuse its diagnostic stream while starting with fewer
configuration mechanisms.

## Implementation choices

All three choices keep hd syntax and runtime behavior unchanged.
They analyze the same call:

```text
struct Point:
    x: i32

impl Point:
    fn translate(&mut self, dx: i32) -> void:
        self.x = self.x + dx

fn make_point() -> Point: Point { x: +0 }

pub fn main() -> void:
    make_point().translate(+1)
```

| Choice | Mechanism | Benefit | Cost or limit |
| --- | --- | --- | --- |
| Typed-call collection | During lint-requested body checking, collect facts at resolved calls; render findings separately. | Retains source receiver identity and resolved signatures. | Initially rechecks selected bodies rather than trusting check-only cache hits. |
| Cached lint facts | Persist policy-independent call facts with each check entry; lint reads those facts. | Warm lint can avoid body checking. | Adds cache schema, invalidation, serialization, and source relocation work before one lint ships. |
| Syntax-only scanner | Scan temporary-looking calls without typed checking. | Removes checker integration and cache design. | Cannot establish receiver permission, result, requirement row, or nested reference freedom; cannot implement this lint faithfully. |

The scanner is the radical simplification, but loses the required precision.
The first two choices add CLI behavior and compiler tooling; neither adds
language syntax, a semantic exception, an intrinsic, or a std API.
Their runtime and RC representation costs are zero because lint emits no Wasm.

| Criterion | Typed-call collection | Cached lint facts | Syntax-only scanner |
| --- | --- | --- | --- |
| Agent JSON and CI | Shared reporter and levels | Same | Same output, unreliable findings |
| Reference-bearing temporary | Skip using resolved types | Skip using stored types/facts | Cannot prove the exclusion |
| Rules | One CLI command, levels, predicate | Same, plus cache contract | Different, weaker predicate |
| Soundness | Fail closed on unresolved facts | Also needs complete invalidation | Does not meet #651 |
| Agent writing / human reading | No new hd forms | No new hd forms | No new hd forms |
| Implementation cost | Bounded collector and command | Collector plus durable data model | Small scanner, unacceptable accuracy |
| Evolution | Measure warm cost before adding persistence | Commits early to a fact schema | Must eventually acquire types |

## Recommendation

Use typed-call collection, with one built-in lint and no persisted lint
facts initially. Resolve types normally, then inspect call headers and
receiver origin; never infer effects by scanning the callee body.

Keep findings outside the compiler's ordinary diagnostic buffer until the
lint command applies policy. `hd check` must neither collect nor report
these findings, including after a previous denied lint run.

Initially reuse parse and interface caches but recheck the selected bodies
when lint facts are unavailable. This gives repeatable cold/warm findings
without changing the check-entry format.
Cached facts are the next option only if measured warm cost warrants them.

This sacrifices an immediately cheap warm lint run. There is no measured
lint speed claim, since the command does not exist.

## Command and configuration

Proposed invocation:

```sh
hd lint
hd lint src/geometry.hd --format json
hd lint --all --deny lost-temporary-write
hd lint -p geometry --allow lost-temporary-write
```

Selection follows `check`: package by default, optional FILE, workspace
members through `-p`, and `--tests` or `--all` for their existing scopes.
Dependencies are checked as needed, but lint findings belong only to the
selected package sources. Generated expansion bodies have no independent finding.

Reuse `--format`, `--summary`, `--max-errors`, and `--jobs` behavior from
`check`. `--allow CODE` and `--deny CODE` accept repeatable, exact lint
names; no groups, prefixes, or plugins initially.

Proposed `hd.toml` configuration:

```toml
[lint]
allow = []
deny = ["lost-temporary-write"]
```

The first lint defaults to warning. Allow suppresses its finding; deny
reports an error. A name in both lists is a configuration error, as is
an unknown name or non-list value.

A full-line `# lint-allow: CODE` immediately before a statement suppresses
that lint only on calls directly in that statement. Nested function and
closure bodies do not inherit it. A preceding allow comment wins over
manifest and CLI deny for that site; unknown codes are configuration errors.

This is a proposed CLI interpretation of an ordinary comment, not new
hd syntax. It provides the owner-requested per-site escape hatch without
adding a decorator, lint group, or body-wide policy mechanism.

CLI settings override manifest settings for the named lint. Contradictory
CLI settings are usage errors, independent of argument order.
Other lints retain their manifest or default level.

Read each selected package's own manifest; do not inherit parent-directory
or workspace lint policy implicitly. A standalone file uses defaults plus
CLI settings. Dependencies' policies cannot change the caller's lint run.

These are proposed CLI-tier rules. Acceptance would require new rule IDs,
manifest validation, help text, and conformance cases together with code.

## First lint

**Checked escape case.** The original header predicate did not prove that
receiver storage becomes unreachable. This checked example saves the
temporary through another argument; Node prints `7` afterward.

```text
struct Point:
    x: i32

impl Point:
    fn remember(&mut self, into: &mut List[&mut Point]) -> void:
        self.x = +7
        into.push(self)

pub fn main() -> void $ Console:
    let mut kept: List[&mut Point] = []
    Point { x: +0 }.remember(&mut kept)
    println("${kept[0].x}")
```

The method returns `void`, has an empty row, and is not bang.
`Point` contains only an integer, yet its write remains observable.
A callee can also save `self` before a later caller mutates it.

The owner tightened the predicate: ordinary parameters must also be free
of references, trait values, and host handles at every depth. Therefore
`remember` is quiet under the revised lint. No loss proof or automatic
fix is required: this remains a likely-bug heuristic.

A method can still save `self` into a mutable top-level binding. That
escape cannot be seen from its header and remains an accepted false-positive
path. The per-site allow comment suppresses the warning when intentional.


Proposed stable code: `lost-temporary-write`.
Report only when every fact below is established:

| Fact | Evidence available at the call |
| --- | --- |
| Receiver is a temporary | The call's automatic address creation refers to a non-place value. Parentheses preserve that classification; they do not prove non-escape. |
| Method writes through its receiver | Resolved receiver permission is `&mut self`. A writable reference returned by another call instead names external storage. |
| Result is `void` | Resolved, substituted signature; no unresolved result variable. |
| No declared requirement | Resolved requirement row is provably empty. |
| No suspension | This is not a bang call. |
| Ordinary parameters hold no observable indirection | After substitution, every parameter other than `self` passes the same recursive type test as the receiver. |
| Receiver holds no observable indirection | Its complete semantic type contains no reference, trait value, or host handle at any depth. |

Walk struct fields, enum variants, tuples, and collection element types
for the receiver and every ordinary parameter after substituting type arguments.
A `List[Point]` can qualify; a
`List[&Point]` cannot. Strings' private RC representation does not make
them semantic references.

Unknown type parameters, opaque external types, unresolved projections,
or function values with unknown captures fail the proof and remain quiet.
Use a visited set for recursive types; an unproved cycle remains unknown.
Recognize host handles from existing type facts, not their visible field count.

The warning concerns a temporary receiver, not a useless call. Receiver
construction and argument evaluation can have effects. The method can mutate
global storage, panic, or diverge. Therefore the finding carries no automatic fix.

Proposed message: "`translate` mutates a temporary receiver; bind the value
if you meant to keep the change".
Use the actual method name and underline the receiver expression.
Do not suggest a fabricated variable name or delete the call.

| Call shape | Proposed result |
| --- | --- |
| `make_point().translate(+1)` | Warning when all signature and type facts above hold. |
| `(make_point()).translate(+1)` | Same warning. |
| `let mut p = make_point(); p.translate(+1)` | Quiet: a writable place. |
| `get_point_ref().translate(+1)` | Quiet: changes an existing referenced value. |
| `Appender { into: &mut xs }.add(+3)` | Quiet: receiver contains a reference. |
| `Point { x: +0 }.remember(&mut kept)` | Quiet: an ordinary parameter contains a reference. |
| A method storing `self` into a mutable top-level binding | Accepted potential false positive; suppress at the site. |
| Pure `void` method on a wrapper containing `List[&Point]` | Quiet: nested reference. |
| A temporary's `void` method with `$ Console` | Quiet: nonempty requirement row. |
| A temporary's method returning a value | Quiet, even when that result is discarded. |
| A bang method, host handle, trait value, or unproved generic receiver | Quiet. |

This remains a conservative likely-bug warning. It does not promise that
a qualifying method actually writes; `&mut self` only grants permission.
That limitation follows #651's header-only condition.

## Output and status

Reuse the existing diagnostic object and final summary without adding a
second JSON dialect. JSON goes to stdout, text diagnostics and summaries
to stderr, as for `check`. Sort findings by package-relative path and span.

Example proposed output for the first `translate` program:

```json
{"kind":"diagnostic","code":"lost-temporary-write","severity":"warning","message":"`translate` mutates a temporary receiver; bind the value if you meant to keep the change","file":"src/main.hd","line":11,"column":5,"fixes":[]}
{"kind":"summary","errors":0,"warnings":1,"passed":0,"failed":0,"ignored":0,"status":0,"modules_checked":1}
```

Under deny, the diagnostic becomes an error, the counts become one error
and zero warnings, and status is 101. Allow emits no diagnostic.
Warnings alone exit 0, preserving the CLI's existing severity convention.

Compiler errors remain normal compiler errors; lint configuration cannot
suppress them. Do not produce a lint from an unresolved or erroneous call.
Usage, configuration, or internal failure exits 101 with the existing summary.

`hd fix` continues to consume compiler fix-its only. This proposal adds
no lint fixing command and no edits to manifests or sources.

## Implementation order and validation

After approval, apply the CLI-tier rules for selection, policy, predicate,
and output. Register the diagnostic in the language-tier README catalogue,
without changing whether any hd program is accepted.
Ship numbered rules with new IDs and conformance fixtures with each implementation slice.

First add the lint command, configuration validation, and shared reporting.
Then collect the first lint's typed-call facts, preserving source spans
before receiver lowering loses their origin. Keep policy out of cache keys
and compiler diagnostics by applying it only to collected findings.

The acceptance matrix includes every call shape above, unknown types,
reference-bearing enum variants, and host handles nested in aggregates.
Exercise inferred rows and defaulted arguments after signature resolution.
Run cold, warm, allow, and deny in varying order; findings must remain equal.

CLI fixtures should compare text and JSON, exact spans, workspace selection,
unknown names, conflicting levels, per-site allow overriding deny, comment
scope across closures, and unchanged `check` output.
No fixture may require reading method bodies to decide whether to warn.
Record cold/warm lint time before proposing persistence; no target is assumed here.

## Evidence and parse log

Source inspection on `9c39889b68a3c09baf9f4ff5efbbcfcdb6327aa4` found reusable
selection in `check_cmd.rs`, rendering in `report.rs`, and the driver's
diagnostic format in `diagnostics.rs`. `Output` has no general typed-call
snapshot API, so the standalone consumer would require a new export contract.

The example uses the existing temporary-receiver rule, not proposed syntax.
The full 11-line example parsed and checked with zero errors and warnings.
These results establish existing program acceptance, not lint implementation
or the proposed finding's correctness.

| Check | Result |
| --- | --- |
| Frozen `parse_check temporary-receiver.hd` | Exit 0; syntax accepted. |
| Frozen `hd check --format json temporary-receiver.hd` | Exit 0; zero errors and warnings, one module checked. |
| Frozen `hd check --format json escaped-receiver.hd` | Exit 0; zero errors and warnings. |
| Frozen `hd escaped-receiver.hd --engine node` | Exit 0; stdout exactly `7\n`. |
| Proposed `hd lint` output | Not executed: the command and lint do not exist. |

The compiler came from `bc3ffb05a3` plus #526's cancellation pruning;
that change alters no syntax or type rule. Parser SHA-256:
`ca6a7ddc0432b75003dab442c5da45356473dd40c531a861054dc6fcd2d448a5`. Compiler SHA-256:
`9b1f42c4c0c62247243594f55a61319e28f09dea3580f89353051b9ff86a9a7c`. Raw commands, outputs, and
provenance are in `/private/tmp/muse-651-evidence-1792771894100412000/checks.json`.
