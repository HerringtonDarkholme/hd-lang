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
| [`findings/`](findings/) | one file per open finding (58), indexed in [`evidence/findings-table.md`](evidence/findings-table.md) | still open on 2026-09-27: the tagged cases still fail, and the others were re-run or spot-checked |
| [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) | `test/portable/KNOWN_FAILURES.tsv` grouped by finding or decision ID | the prototype's fix list |
| [`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/) | minimized fuzz fixtures for F-265, F-310, F-250, and F-252 | open findings; `spec/tools/fuzz/README.md` points here |
| [`evidence/04-runtime/`](evidence/04-runtime/) | replay, host-value, and panic result tables | back F-155, F-161, F-400, F-401, and F-404 |
| [`evidence/05-object-model/`](evidence/05-object-model/SUMMARY.md), [`05-requirements/`](evidence/05-requirements/SUMMARY.md), [`06-compiler/`](evidence/06-compiler/SUMMARY.md) | representation, cost, and compiler-structure measurements | back the architecture review and F-501 to F-612 |
| [`probes/`](probes/), [`scripts/`](scripts/), [`bench/`](bench/) | the inputs and scripts that reproduce those runs | needed to re-run the open findings |
| [`grammar/FINDINGS.md`](grammar/FINDINGS.md) | GR-10 items f and g, GR-21, and the ambiguity tool in [`grammar/tools/`](grammar/tools/) | open reference-parser and teaching findings |
| [`types/QUESTIONS.md`](types/QUESTIONS.md) | type decisions not yet applied (TQ-9 to TQ-12, TQ-15 to TQ-20, TQ-23, TQ-30, TY-13, EQ-1), TQ-13 and TQ-14, and parked TQ-24 to TQ-26 | live owner decisions |
| [`types/FINDINGS.md`](types/FINDINGS.md), [`PROPOSED_RULES.md`](types/PROPOSED_RULES.md), [`RESEARCH.md`](types/RESEARCH.md) | the open type-rule findings, the draft rule text for them, and the language comparison behind them | back the open type questions |

On 2026-09-27 the prototype passes 942 of the 1,085 conformance cases, all of
them selected in `test/portable/cases.tsv`. The other 143 are listed in
`test/portable/KNOWN_FAILURES.tsv`, each tagged with a finding or with a
decision below; all 143 still fail.

## Specification Follow-Ups

- F-150: `typing/invalid/nondisplay-entry-error.hd`,
  `typing/invalid/nonhost-entry-requirement.hd`, and
  `runtime/valid/resource-disposed-result.hd` use a private declaration in
  the signature or requirement row of `pub fn main`, so every conforming
  implementation also reports `private-type-leak`. The fixtures need `pub`
  on those declarations.

## Applied Decisions the Prototype Does Not Follow Yet

Every other applied decision is implemented in the prototype; the spec's
Revision Notes in `spec/README.md` are the record. Three implemented
decisions keep cases in the known failures under a finding: L2's
`unsigned-exponent.hd` and TQ-4's literal-default cases need sized numeric
types (F-253), and A3's optional-to-`Any` case needs `Any` (F-255). Runtime type identity
(Inspectable decisions 1 to 16) is implemented except one case of decision 16
(I16); its alias-and-newtype case
and its local-type `impl Error` case need `type` and local declarations
(F-254).

`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
| TQ-1 | An impl target starts with a type constructor; overlap is decided by trait, unifying trait arguments, and target constructor; `for` accepts `Iterable[T]` or `Iterator[T]` directly. The prototype has no `Iterable` trait, so user `Iterable` impls and `Iterable` bounds fail. |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check is implemented, but its fixtures need `Iterable` or package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| TQ-29 | A type implementing both `Iterable[T]` and `Iterator[T]` is iterated through `Iterable`. The prototype has no `Iterable` trait. |
| EMB-S | Rust-style trait lookup: a trait method is a candidate only where its trait is available, wherever the impl is declared; an unavailable trait is invisible, so a promoted method of that name is selected and a call that finds nothing is `unknown-method` suggesting the import. `member-lookup.ts` and `program-embedding.ts` implement the rest, but the prototype checks one module without trait imports (a multi-file package is linked into one namespace), so every trait is available, and the fixtures need package roles. |
| P2 | Member lookup skips own fields and inherent methods not visible from the calling module; a private member of an embedded type is never promoted, and `private-member` is reported only for an invisible own member when nothing visible matches. `member-lookup.ts` follows the algorithm, but the prototype checks one module (a linked package shares one namespace), so every own member is visible, and the fixtures need package roles. |
| VE | Value embedding. The prototype implements it except the variance rule: an embedded field is invariant for variance, but declared variance markers do not parse (F-705), so `covariant-embedded-field.hd` cannot be checked. |
| R-MUT | A provider installed with `$.with(mut K=value)` may be retrieved as `$.use(mut K)`, and rows carry `mut K`; retrieving or requiring `mut K` where only readonly access is installed is `mutable-upgrade`; a runtime profile binds a host provider `mut` only for a trait it marks mutable. The prototype parser rejects a `mut` requirement key. |
| GQ2 | After an indented closure body inside brackets, the next line must start with `,` or a closing delimiter. The prototype rejects a closing delimiter on a body line, but it parses a closure body nested in brackets as a single statement, so multi-statement bodies fail (two cases, including chapter 07's parenthesized closures), and it does not check how far the following line is indented. |
| GQ4 | `pack.map(` and `pack.map_list(` always form the pack operation, even beside a local named `pack`. The prototype checks the operation's argument shape but has no pack operations, so a valid use still resolves as a method call. |
| GQ11 | `[` directly after `annotate` always opens generic parameters. The prototype has no `annotate` declarations. |
| GQ14 | Annotation bodies accept `pass` alone on an indented line. The prototype has no `annotate` declarations. |
| EC | Error conversion decision 4: a dynamic trait value type satisfies a bound on its own trait and its supertraits. The prototype passes a trait value to a bounded parameter only with a dictionary from an implementation, so it reports `unsatisfied-trait-bound`; supporting it needs a dictionary whose methods forward through the value's own table. |
| I16 | Inspectable decision 16: a `mut` inside a type argument is part of runtime identity. The prototype keeps it for written types, but a type parameter instantiated with `mut U` gets a dictionary built from `U`, so a `List[T]` erased inside the generic records `List[U]`. |
