# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the conformance and
specification work that followed, are finished. Finished items have been
removed from this folder. What remains:

- [`REPORT.md`](REPORT.md): the audit's verdict, architecture review, and the
  findings still open.
- [`findings/`](findings/): one file per open finding (65 as of 2026-09-26).
  Most are prototype compiler bugs or performance notes; the index is
  [`evidence/findings-table.md`](evidence/findings-table.md).
- [`evidence/`](evidence/), [`probes/`](probes/), [`scripts/`](scripts/):
  the runs that back those findings. [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv)
  is the prototype's fix list, grouped by ID.
- [`bench/`](bench/): the small benchmark set kept for future direction.

## Specification Follow-Ups

- F-150: `typing/invalid/nondisplay-entry-error.hd` and
  `nonhost-entry-requirement.hd` use a private declaration in the signature of
  `pub fn main`, so every conforming implementation also reports
  `private-type-leak`. The fixtures need `pub` on those declarations.

## Applied Decisions the Prototype Does Not Follow Yet

G2, G3, L6, L7, L8, L9 are implemented; L2's remaining case
(`unsigned-exponent.hd`) waits on sized numeric types and is tagged F-253.
TQ-3 (inherent before trait methods), TQ-5 (`mutable-impl-target`), and TQ-6
(supertrait member names) are implemented, as are the TQ-1 target and overlap
checks and the TQ-2 ownership check. E1, E3, E4, and E5 are implemented:
own members before promoted ones, breadth-first promotion with
`ambiguous-promoted-member`, no promotion of trait methods, no promoted
methods filling implementations. M2 replaces M1 and is implemented: separate
field and method lookups, same-named fields and methods, and
`(x.callback)(args)` for function-typed fields. TQ-4 (a dot call chooses among
instantiations of one generic trait by argument and expected types), TQ-28
(overlap by unifying full heads), and TQ-27's tuple targets and
`function-impl-target` are implemented. O1 to O3 are implemented as sugar
over the existing erased optional carrier: `Option[T]` is `T?`, `.None`,
`.Some(value)`, and their `Option.`-qualified forms construct and match
optionals, the implicit wrap adds one layer only, and `Option[T]` targets
behave like `T?` targets. P6 is implemented: `Type::name` and `x::name`
without a call are rejected while parsing, a called `x::name(...)` while
checking, both as `deferred-method-value`, and `x.callback(args)` with only a
function-typed field reports `unknown-method` suggesting `(x.callback)(...)`.
The grammar decisions GQ1 (a trailing
requirement clause belongs to the declaration), GQ5 (data patterns label
fields with `:`), and GQ7 (leading-dot continuation) are implemented; GQ2
is implemented only for closure bodies of one statement.
`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
| TQ-1 | An impl target starts with a type constructor; overlap is decided by trait, unifying trait arguments, and target constructor; `for` accepts `Iterable[T]` or `Iterator[T]` directly. The prototype has no `Iterable` trait, so user `Iterable` impls and `Iterable` bounds fail. |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check is implemented, but its fixtures need `Iterable` or package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| TQ-29 | A type implementing both `Iterable[T]` and `Iterator[T]` is iterated through `Iterable`. The prototype has no `Iterable` trait. |
| E2 | An own member blocks promotion whatever its kind or visibility; an invisible one is `private-member`, and a name present only through an unavailable trait is `trait-not-in-scope`. The prototype tracks neither member visibility across modules nor trait availability, and its fixtures need package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| R-MUT | A provider installed with `$.with(mut K=value)` may be retrieved as `$.use(mut K)`, and rows carry `mut K`; retrieving or requiring `mut K` where only readonly access is installed is `mutable-upgrade`, and host providers are readonly. The prototype parser rejects a `mut` requirement key. |
| GQ2 | After an indented closure body inside brackets, the next line must start with `,` or a closing delimiter. The prototype rejects a closing delimiter on a body line, but it parses a closure body nested in brackets as a single statement, so multi-statement bodies fail, and it does not check how far the following line is indented. |
