# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the conformance and
specification work that followed, are finished. Finished items have been
removed from this folder. What remains:

- [`REPORT.md`](REPORT.md): the audit's verdict, architecture review, and the
  findings still open.
- [`findings/`](findings/): one file per open finding (58 as of 2026-09-26).
  Most are prototype compiler bugs or performance notes; the index is
  [`evidence/findings-table.md`](evidence/findings-table.md).
- [`grammar/`](grammar/) and [`types/`](types/): the specification audits of
  roadmap areas 1 and 2, reduced to their open findings and to the type
  decisions not yet applied to the specification.
- [`evidence/`](evidence/), [`probes/`](probes/), [`scripts/`](scripts/):
  the runs that back those findings and the report's scorecard.
  [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv) is the
  prototype's fix list, grouped by ID.
- [`bench/`](bench/): the small benchmark set kept for future direction.

On 2026-09-26 the prototype passes 932 of the 1,074 conformance cases, all of
them selected in `test/portable/cases.tsv`. The other 142 are listed in
`test/portable/KNOWN_FAILURES.tsv`, each tagged with a finding or with a
decision below.

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
(Inspectable decisions 1 to 15) is implemented; its alias-and-newtype case
and its local-type `impl Error` case need `type` and local declarations
(F-254).

`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
| TQ-1 | An impl target starts with a type constructor; overlap is decided by trait, unifying trait arguments, and target constructor; `for` accepts `Iterable[T]` or `Iterator[T]` directly. The prototype has no `Iterable` trait, so user `Iterable` impls and `Iterable` bounds fail. |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check is implemented, but its fixtures need `Iterable` or package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| TQ-29 | A type implementing both `Iterable[T]` and `Iterator[T]` is iterated through `Iterable`. The prototype has no `Iterable` trait. |
| EMB-S | Rust-style trait lookup: a trait method is a candidate only where its trait is available, wherever the impl is declared; an unavailable trait is invisible, so a promoted method of that name is selected and a call that finds nothing is `unknown-method` suggesting the import. `member-lookup.ts` and `program-embedding.ts` implement the rest, but the prototype compiles one module without trait imports, so every trait is available, and the fixtures need package roles. |
| P2 | Member lookup skips own fields and inherent methods not visible from the calling module; a private member of an embedded type is never promoted, and `private-member` is reported only for an invisible own member when nothing visible matches. `member-lookup.ts` follows the algorithm, but the prototype compiles one module, so every own member is visible, and the fixtures need package roles. |
| VE | Value embedding. The prototype implements it except the variance rule: an embedded field is invariant for variance, but declared variance markers do not parse (F-705), so `covariant-embedded-field.hd` cannot be checked. |
| R-MUT | A provider installed with `$.with(mut K=value)` may be retrieved as `$.use(mut K)`, and rows carry `mut K`; retrieving or requiring `mut K` where only readonly access is installed is `mutable-upgrade`; a runtime profile binds a host provider `mut` only for a trait it marks mutable. The prototype parser rejects a `mut` requirement key. |
| GQ2 | After an indented closure body inside brackets, the next line must start with `,` or a closing delimiter. The prototype rejects a closing delimiter on a body line, but it parses a closure body nested in brackets as a single statement, so multi-statement bodies fail (two cases, including chapter 07's parenthesized closures), and it does not check how far the following line is indented. |
| GQ4 | `pack.map(` and `pack.map_list(` always form the pack operation, even beside a local named `pack`. The prototype checks the operation's argument shape but has no pack operations, so a valid use still resolves as a method call. |
| GQ11 | `[` directly after `annotate` always opens generic parameters. The prototype has no `annotate` declarations. |
| GQ14 | Annotation bodies accept `pass` alone on an indented line. The prototype has no `annotate` declarations. |
| EC | Error conversion decision 4: a dynamic trait value type satisfies a bound on its own trait and its supertraits. The prototype passes a trait value to a bounded parameter only with a dictionary from an implementation, so it reports `unsatisfied-trait-bound`; supporting it needs a dictionary whose methods forward through the value's own table. |
