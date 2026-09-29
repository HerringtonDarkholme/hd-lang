# Error Conversion: The `@error` Intrinsic

Status: decided, not yet applied. Nothing here is accepted behavior until
the specification states it. Owner decisions 1-9, 11, and 13-20 of this
record (2026-09-26 and 2026-09-27) are applied, or superseded by the
testing redesign, and the specification is authoritative for them:
[Propagation](../spec/05-expressions.md#propagation),
[Conversion Trait](../spec/09-traits.md#conversion-trait),
[Error Trait](../spec/09-traits.md#error-trait),
[Dynamic Trait Values](../spec/09-traits.md#dynamic-trait-values),
[Variant Constructors As Function Values](../spec/08-data-and-enums.md#variant-constructors-as-function-values),
[Generic Function Values](../spec/07-functions.md#generic-function-values),
[Entry Results](../spec/10-modules.md#entry-results),
[Exit Status](../spec/10-modules.md#exit-status), and
[Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks).
The error-chain helpers (`Context`, `.context`, `chain`, `find`,
`root_cause`, `ErrorReport`) are library API in
[Standard Library Design](STDLIB.md#stderror). The survey, the candidate
designs, and the error stress test behind the decisions are in git history.

What remains is decision 10, the `@error` intrinsic, and the part of
decision 12 that depends on it. Chapter 14 mentions `@error` only in prose
([Prefix Decorators](../spec/14-annotations.md#prefix-decorators)), and
Decorators D6 keeps `@derive` and `@error` as compiler intrinsics. The
decision's revisions are kept in order: the spelling paragraph near the
end supersedes the earlier `@derive(Error)` and `@message` spellings.

## Owner Decisions

Not yet applied:

10. **Question 11: error derivation is one compiler intrinsic,
    `@derive(Error)`** (revised twice on 2026-09-27; originally "derived
    `From` implementations wait for typed derivation"). Typed derivation
    deliberately does not cover impl families (one impl per variant, each a
    different trait instantiation), and error enums need `Display`,
    `Error::cause`, and `From` from the same per-variant markers, so they
    are one intrinsic, the equivalent of Rust's `thiserror`, rather than a
    proc-macro-like mechanism. On an enum (or a data type) that says
    `@derive(Error)`, the compiler generates `impl Display`, `impl Error`
    (with `cause()`), and one `impl From[P] for E` per `@from` variant.
    Markers, recognized only under `@derive(Error)`:
    - `@message("... $member ...")` on a variant, or on a data type: the
      `Display` text (see E1 below). A variant without `@message` displays as its variant name.
    - **`@from`** on a one-payload variant (explicit, as in thiserror;
      the automatic `From` and automatic cause of an earlier revision the
      same day were withdrawn after the error stress
      test: automatic `From` could not be turned
      off, an upstream type starting to implement `Error` silently changed
      downstream derives, and adding a variant could remove an existing
      `From`): generates `impl From[P] for E` (pure, as 09 requires) and
      makes the payload the cause. Two `@from` variants with the same
      payload type are an error naming both. A payload whose type is a bare
      type parameter may not be `@from` (review R6: it would overlap every
      concrete `From` under 09 Overlap, as thiserror's `#[from]` on a
      generic field does in Rust, E0119); use `map_err(AppError.Inner)`.
      A `@from` variant whose enum has a common field without a default is
      an error naming the field (review R12).
    - **`@source`** on a one-payload variant, on one payload parameter of a
      multi-member variant (`Parse(path: string, @source error:
      SyntaxError)`, decision 12), or on a data-type error's field: that
      member is the cause, with no `From`; its type is `E < Error` or `E?`
      (an absent optional cause gives `.None`). There is no automatic cause
      and no field-name convention: a variant without `@from` or `@source`
      has no cause. At most one `@from`/`@source` member per variant.
    - `@transparent` on a one-payload variant: `Display` and `cause()`
      forward to the payload.
    Refinements decided 2026-09-27: (E1) a `@message` text is an ordinary
    hd interpolated string (`$name`, `${expression}`) with the variant's
    payload members and the enum's common fields in scope, so placeholder
    checking is ordinary type checking (an unknown name is `unknown-name`;
    an interpolated member must implement `Display`; `$self` in a message is
    a compile error, because it would recurse into the generated `Display`,
    review R7); (E2) `Display` is
    always generated, so a hand-written `Display` cannot be combined with
    `@derive(Error)`; (E3) a `@transparent` variant's `cause()` returns the
    inner error's cause, as thiserror does, so `chain` does not repeat the
    inner message; consequently `find[Inner]()` does not see the
    transparent inner error itself, exactly as in Rust (confirmed in
    review R10: follow thiserror); (E4) see `@source` above; common enum fields cannot be the cause.
    (Gap 1, decided 2026-09-27) Bounds of the generated impls for a
    generic error type are inferred per use, as thiserror does: `Display`
    gets `P < Display` for each type parameter `P` whose value a message
    interpolates (or that a `@transparent` variant forwards to), and `Error`
    gets `P < Error` for each type parameter that is the type of a
    `@source`/`@from` member; a parameter that is only carried gets no
    bound. Tooling (`hd doc`) shows the inferred bounds.
    (Gap 2, decided 2026-09-27) `@message`, `@from`, `@source`, and
    `@transparent` are derive helper markers, as in Rust's derive helper
    attributes: they exist only inside an item that says `@derive(Error)`,
    are not names, cannot be imported, and cannot be shadowed by imports;
    outside such an item they have no special meaning.
    (Gap 3, decided 2026-09-27) Inside a variant's `@message`, unnamed
    payload parameters are in scope as `_0`, `_1`, ... (zero-based, in
    declaration order; unnamed parameters come first), so
    `@message("not found: $_0")` on `NotFound(string)` works with ordinary
    `$identifier` interpolation. Named members are in scope by name. The
    spelling matches tuple access, `pair._0` (audit TUP-1, same day;
    applied in
    [Parenthesized And Tuple Expressions](../spec/05-expressions.md#r-expr.tuple.select-underscore)).
    `@source` on an unnamed parameter needs no name
    (`Io(@source FsError)`, decision 12).
    (Gap 4, decided 2026-09-27) Opaque public errors: `@error(transparent)`
    and `@from` are allowed on a one-field data type, so a stable public
    type can hide an internal error enum (thiserror's
    `#[error(transparent)] pub struct PublicError(#[from] ErrorRepr)`).
    (Spelling, decided 2026-09-27; supersedes the spellings above) The
    intrinsic is the annotation `@error`, not `@derive(Error)`: `@error` on
    an enum makes it a derived error; `@error("...")` on a variant is its
    message (formerly `@message`); `@error("...")` on a data type is both
    the trigger and the message; `@error(transparent)` on a variant or a
    one-field data type replaces `@transparent`; `@from` and `@source` stay,
    written on the payload parameter or field (`Yaml(@from error:
    YamlError)`, decision 12). `@error` is a compiler intrinsic like
    `@derive`, so an imported `error` cannot shadow it; inside an `@error`
    item, `transparent`, `from`, and `source` are markers, not names (gap
    2). `@derive` stays the closed list of comparison and hash intrinsics;
    `Error` is not added to it.
    Scope: the intrinsic is Rust's `thiserror` moved into hd (messages,
    `@from`, `@source`, `@transparent`) and no more. It has no error codes:
    inside a program the typed variant is the code (`find[T]()` then
    `match`), and codes for logs and APIs belong to the boundary-safe
    report type of decision 6 (miette keeps codes on a separate
    `Diagnostic` trait for the same reason).
    Generated impls are ordinary impls: a hand-written duplicate is
    `overlapping-impl`. Unmarked variants get no `From`, so
    `Invalid(reason: string)` never yields `From[string]`. `@error` is its
    own intrinsic; `@derive`'s closed list (TQ-13) does not gain `Error`.
    Reference case: ast-grep's
    `RuleCoreError` (three variants carry `RuleSerializeError`; one is
    `@from`, two are `@source`). The general fact check hook (a fact type's
    compile-time `check` against its member or variant) stays for other
    libraries' facts (derivation stress-test P14). Not yet applied to the
    spec.

    Later the same day, Enum Semantics decision 4 made shared enum
    constructor data per-variant constants, never stored in values
    ([Shared Enum Constructor Data](../spec/08-data-and-enums.md#shared-enum-constructor-data)).
    That makes the review R12 rule above about common fields moot: a
    `@from` variant needs no common-field default. Per-value common data,
    such as a span on every error, belongs in each variant's payload or in
    a wrapper data type.

12. **Annotations on enum payload parameters** (2026-09-27, review F1):
    the grammar accepts annotations before a payload parameter,
    `Parse(path: string, @source error: SyntaxError)`, as it already does
    on data field lines. Applied 2026-09-27 with typed derivation: the
    grammar in 02
    ([`grammar.enum.payload-decorator`](../spec/02-grammar.md#r-grammar.enum.payload-decorator))
    and payload facts in 14
    ([`annot.fact.payload`](../spec/14-annotations.md#r-annot.fact.payload));
    08 needed no change. What `@from` and `@source` mean there comes with
    `@error` (decision 10), which is not yet applied.

## Current Design

The `@error` design as decided on 2026-09-27, in one place. When a decision
changes it, update this section in the same change. The example parses
with the [reference parser](../spec/reference-parser/index.ts), including
the annotations on enum payload parameters (decision 12).

```text
use std.error.Error

@error
pub enum FsError:
    @error("not found: $path")
    NotFound(path: string)
    @error("permission denied: $path")
    Denied(path: string)

@error
pub enum RuleCoreError:
    @error("Fail to parse yaml as RuleConfig")
    Yaml(@from error: YamlError)
    @error("`utils` is not configured correctly.")
    Utils(@source error: RuleSerializeError)
    @error("`rule` is not configured correctly.")
    Rule(@from error: RuleSerializeError)
    @error("Undefined meta var `$var` used in `$context`.")
    UndefinedMetaVar(var: string, context: string)

@error
pub enum LoadError:
    @error("$path:$line: invalid rule")
    Parse(path: string, line: i64, @source error: SyntaxError)
    @error("cannot read $path")
    Read(path: string, @source error: FsError?)
    @error(transparent)
    Rules(@from error: RuleCoreError)

@error(transparent)
pub data PublicError:
    @from
    repr: LoadError

@error("config $name is missing")
pub data MissingConfig:
    name: string

fn read_rules(path: string) -> Result[string, RuleCoreError]:
    .Err(RuleCoreError.UndefinedMetaVar(var="A", context="rule"))

fn load(path: string) -> Result[string, LoadError]:
    text := read_rules(path)?
    .Ok(text)

fn run(path: string) -> Result[void, Error]:
    text := load(path)?
    .Ok()
```

- **`@error`** (decision 10) is a compiler intrinsic, Rust's `thiserror`
  moved into hd, on an enum or a data type. It generates `impl Display`
  (always; a variant without a message displays as its name),
  `impl Error` with `cause()`, and `From` conversions. `@error("...")` on a
  variant (or a data type) is the message: an ordinary hd interpolated
  string with the payload members (unnamed ones as `_0`, `_1`) and common
  fields in scope. `@from` on the payload of a one-payload variant (or the
  field of a one-field data type) generates `From` and makes it the cause;
  `@source` marks a cause without `From`. `@error(transparent)` forwards
  message and `cause()` (to the inner error's cause). Generated bounds are
  inferred per use. No error codes.
- **Generic error types:** a generated impl for `enum AppError[E]` gets
  `E < Error` for a `@from`/`@source` member of type `E` and `E < Display`
  when a message interpolates it, per used parameter (typed derivation
  M12's rule).

## Still To Do

- Specify `@error` (decision 10) and what `@from` and `@source` mean on a
  payload parameter (decision 12). Decision 12's grammar is already applied
  ([`grammar.enum.payload-decorator`](../spec/02-grammar.md#r-grammar.enum.payload-decorator),
  [`annot.fact.payload`](../spec/14-annotations.md#r-annot.fact.payload)).
- The same pass replaces the prose mention of `@error` in
  [Prefix Decorators](../spec/14-annotations.md#prefix-decorators) with
  rules. This was the one point the Decorators apply pass left open.
