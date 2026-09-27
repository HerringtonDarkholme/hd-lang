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
checks and the TQ-2 ownership check. E1, E4, and E5 are implemented: own
members before promoted ones, no promotion of trait methods, no promoted
methods filling implementations. Cut 2's depth semantics are implemented:
the shallowest member wins, and `program-embedding.ts` reports two members
with one name at the smallest depth as `ambiguous-promoted-member` at the
data declaration; the embedding limits (`too-many-embedded-fields`,
`embedding-too-deep`) are checked there too. The single view is implemented:
the parser reads `pub` on named fields and inherent methods, only `pub`
members of parts are promoted, and a private own member with the name of a
promoted member is `ambiguous-promoted-member` on that member. Parts' trait methods are ignored, and `unknown-method`
suggests the explicit path. Rust-style trait lookup is implemented: only
available trait methods are candidates, and a trait candidate beside a
promoted method is `ambiguous-method`. Trait delegation
(`impl Trait for C by E`) is implemented as generated forwarding methods for
non-generic traits. Embedded fields are always public:
the prototype parser rejects `pub` on an embedded field, and a public data
type embedding a private one is `private-type-leak`. Context spreads are suffix spreads
(`$.with(ctx...)`) in the prototype parser, and the prefix form is a
`syntax-error`. M2 replaces M1 and is implemented: separate
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
function-typed field reports `unknown-method` suggesting `(x.callback)(...)`. The TQ-4
follow-ups (numeric literals prefer their default type among fitting
instantiations; a no-fit `type-mismatch` lists the instantiations) and
`trait-value-impl-target` are implemented; the literal-default fixtures need
sized numeric types and are tagged F-253.
The grammar decisions GQ1 (a trailing
requirement clause belongs to the declaration), GQ5 (data patterns label
fields with `:`), and GQ7 (leading-dot continuation, with its refinement) are
implemented; GQ2 is implemented only for closure bodies of one statement.
GQ3 (nested suite indentation), GQ6 (raw identifiers), GQ8 (bracket suffixes
on a new line), GQ9 (no same-line `if` directly in a same-line suite), GQ10
(trailing blocks after `=`, `_ :=`, `return`, `break`), GQ12 (`name![T](...)`
bang calls), GQ13 (list suffix spreads, lowered to a comprehension), GQ15
(`"$self"`), and GQ17 (contextual `reified`, `super`, `as`, `use`) are
implemented. GQ3 compares a nested body with the statement's first line even
when its header sits on a body line of an outer nested suite, and GQ12 still
accepts the former `value.method[T]!(...)` spelling of a method bang call.
The grammar follow-ups B7 (`unexpected-indentation` for a nested body that
is not deep enough) and B8 (a lone `reified` is a `syntax-error`) are
implemented in both parsers, and B9 added six passing fixtures to the
portable selection. The Option follow-ups A2 (no least-common-type case for
`.None`, which the prototype never had) and A3 are implemented: `is` accepts
optionals, treating every `.None` as one identity, and optionals satisfy
`Reference`; erasing an optional to `Any` is tagged F-255 because the
prototype has no `Any`. The prototype-fix follow-ups C1 to C3 are
implemented: the lexer reports `0b1z` as `syntax-error`, and an impl
method's generic parameters are compared with the trait method's by
position, bounds included.
`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
| TQ-1 | An impl target starts with a type constructor; overlap is decided by trait, unifying trait arguments, and target constructor; `for` accepts `Iterable[T]` or `Iterator[T]` directly. The prototype has no `Iterable` trait, so user `Iterable` impls and `Iterable` bounds fail. |
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. The check is implemented, but its fixtures need `Iterable` or package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| TQ-29 | A type implementing both `Iterable[T]` and `Iterator[T]` is iterated through `Iterable`. The prototype has no `Iterable` trait. |
| EMB-S | Embedding simplification. Rust-style trait lookup: a trait method is a candidate only where its trait is available, wherever the impl is declared; an unavailable trait is invisible, so a promoted method of that name is selected and a call that finds nothing is `unknown-method` suggesting the import. `member-lookup.ts` and `program-embedding.ts` implement the rest, but the prototype compiles one module without trait imports, so every trait is available, and the fixtures need package roles (`--package-role`, `--dependency`), which the prototype lacks. |
| P2 | Member lookup skips own fields and inherent methods not visible from the calling module; a private member of an embedded type is never promoted, and `private-member` is reported only for an invisible own member when nothing visible matches. `member-lookup.ts` follows the algorithm, but the prototype compiles one module, so every own member is visible, and the fixtures need package roles. |
| VE | Value embedding: filling an embedded field (literal `Label: ...value`, copy-update, store `place ...= value`; plain forms are `embedded-copy-required`, and `...` or `...=` elsewhere is `copy-into-ordinary-field`) stores a copy, shallow for ordinary fields and recursive for embedded parts; access through an embedded field follows its container, so a promoted `mut self` method works on a `mut` receiver; reading a part out aliases it; a readonly copy of a part whose type has direct `mut U` fields is readonly; a part is copied at its field position; an embedded field is invariant for variance. The prototype implements all of it except the variance rule: the parser and checker handle both copy forms, `dataMember` gives an embedded field its container's access, `hasMutableEdges` decides the copy's access, and the emitter generates one `$hd.copy_d<N>` function per embedded data type. Declared variance markers do not parse (F-705), so `covariant-embedded-field.hd` stays; one fixture with `i64` is tagged F-253. |
| R-MUT | A provider installed with `$.with(mut K=value)` may be retrieved as `$.use(mut K)`, and rows carry `mut K`; retrieving or requiring `mut K` where only readonly access is installed is `mutable-upgrade`; a runtime profile binds a host provider `mut` only for a trait it marks mutable. The prototype parser rejects a `mut` requirement key. |
| GQ2 | After an indented closure body inside brackets, the next line must start with `,` or a closing delimiter. The prototype rejects a closing delimiter on a body line, but it parses a closure body nested in brackets as a single statement, so multi-statement bodies fail, and it does not check how far the following line is indented. |
| GQ4 | `pack.map(` and `pack.map_list(` always form the pack operation, even beside a local named `pack`. The prototype checks the operation's argument shape but has no pack operations, so a valid use still resolves as a method call. |
| GQ11 | `[` directly after `annotate` always opens generic parameters. The prototype has no `annotate` declarations. |
| GQ14 | Annotation bodies accept `pass` alone on an indented line. The prototype has no `annotate` declarations. |
