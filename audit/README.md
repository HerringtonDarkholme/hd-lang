# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the grammar and type
audits that followed, are finished. Everything they found that has since been
fixed, applied to the specification, superseded, or dropped has been removed
from this folder. The spec's Revision Notes in
[`spec/README.md`](../spec/README.md#revision-notes) record the applied
decisions, and the repository history keeps the removed evidence.

## What Remains

| Path | What it holds | Why it stays |
| ---- | ------------- | ------------ |
| [`REPORT.md`](REPORT.md) | the architecture review and the ranked open findings | the review still describes the prototype |
| [`findings/`](findings/) | one file per open finding (42), indexed in [`evidence/findings-table.md`](evidence/findings-table.md) | still open on 2026-09-27: the tagged cases still fail, and the others were re-run or spot-checked |
| [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) | `test/portable/KNOWN_FAILURES.tsv` grouped by finding or decision ID | the prototype's fix list |
| [`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/) | minimized fuzz fixtures for F-265, F-310, and F-250 | open findings; `spec/tools/fuzz/README.md` points here |
| [`evidence/04-runtime/`](evidence/04-runtime/) | replay, host-value, and panic result tables | back F-155, F-161, F-401, and F-404 |
| [`evidence/05-object-model/`](evidence/05-object-model/SUMMARY.md), [`05-requirements/`](evidence/05-requirements/SUMMARY.md), [`06-compiler/`](evidence/06-compiler/SUMMARY.md) | representation, cost, and compiler-structure measurements | back the architecture review and F-501 to F-612 |
| [`probes/`](probes/), [`scripts/`](scripts/), [`bench/`](bench/) | the inputs and scripts that reproduce those runs | needed to re-run the open findings |
| [`grammar/FINDINGS.md`](grammar/FINDINGS.md) | GR-10 item f, GR-21, and the ambiguity tool in [`grammar/tools/`](grammar/tools/) | open reference-parser and teaching findings |
| [`types/QUESTIONS.md`](types/QUESTIONS.md) | type decisions not yet applied (TQ-18 and the `@message` part of TUP-1), TQ-13 and TQ-14, and parked TQ-24 to TQ-26 | live owner decisions |
| [`types/FINDINGS.md`](types/FINDINGS.md), [`PROPOSED_RULES.md`](types/PROPOSED_RULES.md), [`RESEARCH.md`](types/RESEARCH.md) | the open type-rule findings, the draft rule text for them, and the language comparison behind them | back the open type questions |

On 2026-09-28 the prototype passes 1,157 of the 1,223 conformance cases, all of
them selected in `test/portable/cases.tsv`. The other 66 are listed in
`test/portable/KNOWN_FAILURES.tsv`, each tagged with a finding or with a
decision below; all 66 still fail.

## Specification Follow-Ups

- F-150: `typing/invalid/nondisplay-entry-error.hd`,
  `typing/invalid/nonhost-entry-requirement.hd`, and
  `runtime/valid/resource-disposed-result.hd` use a private declaration in
  the signature or requirement row of `pub fn main`, so every conforming
  implementation also reports `private-type-leak`. The fixtures need `pub`
  on those declarations.

## Applied Decisions the Prototype Does Not Follow Yet

Every other applied decision is implemented in the prototype; the spec's
Revision Notes in `spec/README.md` are the record. Two implemented
decisions keep cases in the known failures under F-253 because they need
sized numeric types: L2's `unsigned-exponent.hd` and TQ-4's
`generic-trait-literal-default-instantiation.hd`.

`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check and `Iterable` are implemented; one fixture needs package roles (`--package-role`, `--dependency`), which the prototype CLI lacks, and `trait-argument-owner-impl.hd` builds `mut Word` comprehension elements where its result type asks for `Word`. |
| EMB-S | Rust-style trait lookup: a trait method is a candidate only where its trait is available, wherever the impl is declared; an unavailable trait is invisible, so a promoted method of that name is selected and a call that finds nothing is `unknown-method` suggesting the import. `member-lookup.ts` and `program-embedding.ts` implement the rest, but the prototype checks one module without trait imports (a multi-file package is linked into one namespace), so every trait is available, and the fixtures need package roles. |
| P2 | Member lookup skips own fields and inherent methods not visible from the calling module; a private member of an embedded type is never promoted, and `private-member` is reported only for an invisible own member when nothing visible matches. `member-lookup.ts` follows the algorithm, but the prototype checks one module (a linked package shares one namespace), so every own member is visible, and the fixtures need package roles. |
| GQ4 | `pack.map(` and `pack.map_list(` always form the pack operation, even beside a local named `pack`. The prototype checks the operation's argument shape but has no pack operations, so a valid use still resolves as a method call. |
| GQ14 | Annotation bodies accept `pass` alone on an indented line. The prototype has no `annotate` declarations. |
| T49 | Testing T49: `std.testing` declares `snapshot(text, expect="")` and `snapshot_file(text)`. The prototype checks `snapshot` as a string `assert_equal`, with the literal `expect` rule, and has no `snapshot_file`. |

The testing redesign (Testing T2-T31 and T40-T47) passes its fixtures, with
gaps that no fixture reaches: only functions of a `tests:` block are hidden
from code outside it, test modules, integration tests, and test
dependencies are not implemented (a `tests` use root is always
`test-only-use`, since the prototype compiles no integration test module), every test case shares one
instance (F-403), a test body's `Result` reports only its outer tag, not
its `.Ok` value's `ExitCode` (Testing T8), `timeout` is parsed but not enforced, `it_each` runs its
rows as one test case, and `it_prop` and `it_prop_with` (T36) are not
implemented. `hd check` without `--tests` (T42) skips test cases and
test-only functions, but still reports the test-case errors its parser
finds. A trailing block binds the final parameter (T40) only for calls
that the checker plans, not for the built-in functions it special-cases.

`Console` is checked as the prelude trait of Mutable Host Providers
(MHP-1), and its three fixtures pass. The prototype runs only the host
console: `hd run`, `hd test`, and `hd build` report a program-defined
provider as `unsupported-console-provider` and a direct `write_line!` call
as `unsupported-console-call`. How `println` calls a provider's
`write_line!` is an owner question in
[Mutable Host Providers](../future-work/OPEN_ISSUES.md#mutable-host-providers).

`Debug` (Testing T33, T39, and T48) is checked: it is a prelude trait,
`std` supplies it for the primitives and the built-in composites,
`@derive(Debug)` generates an implementation, and `assert_equal` and
`debug` require it. The spec leaves `DebugWriter`'s builder calls and the
`debug` layout to the standard library, so `DebugWriter` has no members, a
derived `debug` writes nothing and does not check its members, and the run
commands report a `debug` call as `unsupported-debug-render`. The
builder API is an owner question in
[Testing Still Open](../future-work/TESTING.md#still-open).
