# Error Conversion

Status: design record for
[Roadmap area 4](ROADMAP.md#4-standard-library), with one type-checking
change for [area 2](ROADMAP.md#2-type-checking-rules). The owner decided the
questions on 2026-09-26 ([Owner Decisions](#owner-decisions)), and the
language parts are applied to the [specification](../spec/README.md); library
API stays in [Standard Library Design](STDLIB.md).
[Still To Do](#still-to-do) lists what is not settled. The survey, candidates,
and questions below are kept as the record of how the decisions were reached;
the sections on the specification describe it as it was before them.

The standard-library draft fixes one error enum per domain (`FsError`,
`HttpError`, `ConsoleError`; [STDLIB owner decision 6](STDLIB.md#owner-decisions)).
It has no story for code that touches two domains. This document surveys
how other languages solve that, compares five candidate designs for hd, and
ends with a recommendation and questions for the owner.

Code sketches use `List[T]` and `Map[K, V]` and uppercase generic parameter
names, following the owner's naming update. Every block parses with the
[reference parser](../spec/reference-parser/index.ts) unless it is marked
**hypothetical** (new syntax the parser rejects today); four blocks are. A
block that parses is not necessarily type-correct today; the text says which
rules it needs.


## Owner Decisions

Decided 2026-09-26:

1. **Question 1: "accepts" means single-step assignability.** `?` propagates
   an error whose type is assignable in one step to the enclosing function's
   error type.
2. **Question 2: a general `From[T]` trait, not a narrow `FromError`.** When
   assignability fails, `?` converts the error through the target error
   type's `From[E]` implementation. `From` is an ordinary conversion trait
   that other code may also call. Whether a `From` implementation may have a
   requirement row or suspend, and whether `?` ever chains conversions, were
   left open here and settled by decision 5.
3. **Questions 3 and 4: the erased application error is the dynamic trait
   value `Error`,** and `std` ships `.context("...")`, `Context`, and cause
   `chain`.
4. **Question 5: a dynamic trait value satisfies bounds on its own trait
   and its supertraits.**
5. **`From` is pure and `?` converts at most once.** A `From`
   implementation has an empty requirement row and does not suspend; `?`
   never chains conversions.
6. **Question 6: an erased `Error` never crosses a registered boundary;**
   code converts it explicitly to a domain enum or an `ErrorReport` value.
7. **Question 8: a variant constructor with exactly one payload field is a
   function value** (`SyncError.Fs` has type `fn(FsError) -> SyncError`), so
   `map_err(SyncError.Fs)` works. This belongs with method values (P6) and
   is cross-referenced there.
8. **Question 9: no mapping clause on `?`;** `From` and `map_err` cover it.
9. **Question 10: no anonymous error unions.**
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
      same day were withdrawn after the [error stress
      test](ERROR_STRESS_TEST.md): automatic `From` could not be turned
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
11. **Location: `From[T]` is declared in `std.convert` and the erased error
    trait in `std.error`.** Neither is a prelude name (consistent with
    [STDLIB decision 7](STDLIB.md#owner-decisions)); code imports them with
    `use std.convert.From` and `use std.error.Error`. `?` uses `From` without
    the caller importing it.
12. **Annotations on enum payload parameters** (2026-09-27, review F1):
    the grammar accepts annotations before a payload parameter,
    `Parse(path: string, @source error: SyntaxError)`, as it already does
    on data field lines. Not yet applied to 02 or 08.
13. **A failing entry point prints the error chain** (2026-09-27, review
    R2) when its error type implements `Error`; otherwise it prints
    `Display.to_string` as today. Applied 2026-09-27 (see below).
14. **`?` in test blocks** (2026-09-27, review R1): a `test` block is a
    propagation target as if it returned `Result[void, Error]`; `.Err` fails
    the test and prints the chain. Applied 2026-09-27 (see below).
15. **`std.error` sketches `.context(...)` and `ErrorReport`** (2026-09-27,
    review R3): `fn context[T, E < Error](self: Result[T, E], message:
    string) -> Result[T, Error]` producing a `Context { message, cause }`
    error, and `ErrorReport { message, causes }` with
    `report_of(error: Error) -> ErrorReport` as the boundary snapshot.
    Library API (STDLIB), not specification text.
16. **`?` in a test block accepts any `E < Display`** (2026-09-27,
    Error Stress Test problem 11): besides errors that reach the block's
    `Error` by decision 14, an `.Err` of any `E < Display` (for example
    `Result[T, string]`) is wrapped in a std message error, so tests of
    string-error helpers can use `?`. This is a test-block rule only.
17. **`std.process.ExitStatus` chooses the exit code** (2026-09-27, problem
    12): `trait ExitStatus: fn status(self) -> i32`. When `main` returns
    `.Err(e)` and `E` implements `ExitStatus`, the process exits with
    `e.status()`; otherwise it exits with 1. It controls the code only; the
    entry point still prints the error (decision 13). A tool that must exit
    silently prints and exits by hand.
18. **Erased errors stay off boundaries** (2026-09-27, problem 13): no new
    rule. A registered function's error type holds only boundary-safe
    payloads; an error type with an erased `Error` member converts with
    `report_of` to `ErrorReport` first. The error-handling guide shows the
    pattern.
19. **Generic function values infer their type arguments at the use site**
    (2026-09-27, problem 15): a generic function or generic enum
    constructor passed as an argument has its type arguments solved together
    with the call's other type variables, so
    `result.map_err(TaskError.Failed)` on `Result[T, FsError]` infers
    `TaskError[FsError]`. A parameter left unsolved is an error. This
    changes 07 `r[fn.type.generic.instantiate]` and 08
    `r[data.enum.fn-value.generic]`, which demanded a complete expected
    type. The owner's no-inference rule is about declarations: a function
    declaration's own generic parameters and signature are written, never
    inferred; use sites may infer.
20. **Non-exhaustive enums: none for now** (problem 16): adding a variant is
    a breaking change ([Packages decision 7](PACKAGES.md)); library authors
    who need to grow a kind enum use the private-field wrapper idiom.

### Applied To The Specification

Applied 2026-09-26:

- **Decisions 1, 2, and 5:** [Propagation](../spec/05-expressions.md#propagation)
  defines the one-step rule: assignability by one rule, otherwise one call of
  the target's `From[E]` implementation, never both and never chained;
  anything else is `invalid-result-propagation`.
  [Result Types](../spec/04-type-system.md#result-types) points to it.
  [Conversion Trait](../spec/09-traits.md#conversion-trait) declares
  `From[T]` with the purity rule: a `from` with a requirement clause or `!`
  does not match the trait method and is `trait-method-signature`, so no new
  diagnostic code was needed. It also covers ordinary coherence and direct
  `Target::from(x)` calls, which choose among instantiations like a dot call.
- **Decision 3 (language part) and decision 6:**
  [Error Trait](../spec/09-traits.md#error-trait) states that `std.error`
  declares `Error < Display`, that every member has a default, that the
  dynamic value `Error` is the erased application error, and that it never
  crosses a registered boundary ([Wasm Boundary](../spec/10-modules.md#wasm-boundary)).
  `.context`, `Context`, `cause`, and `chain` stay library API in
  [STDLIB](STDLIB.md#stderror).
- **Decision 4:** [Dynamic Trait Values](../spec/09-traits.md#dynamic-trait-values)
  states that a dynamic trait value type satisfies bounds on its own trait
  and its supertraits; [Executable Entry Point](../spec/10-modules.md#executable-entry-point)
  notes that `Result[void, Error]` is a valid entry result.
- **Decision 7:** [Enum Declarations](../spec/08-data-and-enums.md#enum-declarations)
  makes a single-payload variant constructor a function value;
  [Function Types And Values](../spec/07-functions.md#function-types-and-values)
  and [Unsupported Function Extensions](../spec/07-functions.md#unsupported-function-extensions)
  cross-reference it from the method-value deferral (P6). Two or more
  payload fields stay `unsaturated-enum-constructor`.
- **Decisions 8 and 9:** listed as unsupported extensions in
  [Expressions](../spec/05-expressions.md#unsupported-expression-extensions)
  and [Type System](../spec/04-type-system.md#unsupported-type-system-extensions).
- **Decision 11:** [Prelude](../spec/10-modules.md#prelude) and the two
  trait sections name the modules.
- **Recovery from an erased `Error`** (applied with runtime type identity,
  2026-09-26): [Error Trait](../spec/09-traits.md#error-trait) declares
  `Error < Display + Inspectable`, so the inherited default method
  `error.downcast[FsError]()` recovers a concrete error
  ([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)).
  `error.find[T]()`, `chain`, and `root_cause` are library API in
  [STDLIB](STDLIB.md#stderror).

Applied 2026-09-27:

- **Decision 13:** [Entry Results](../spec/10-modules.md#entry-results)
  prints an `Error` chain, the message and then each cause as
  `caused by: ...` (`r[module.entry.err-render-chain]`); other error types
  still print `Display.to_string`.
- **Decisions 14 and 16:** [Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks)
  makes a `test` block a propagation target as if it returned
  `Result[void, Error]`, wraps any other `E < Display` in a std message
  error, and fails the test on a propagated `.Err`
  (`r[expr.try.target.test]`, `r[expr.try.test.display]`).
- **Decision 17:** [Exit Status](../spec/10-modules.md#exit-status)
  declares `std.process.ExitStatus` and uses `error.status()`, otherwise 1
  (`r[module.entry.exit-status]`).
- **Decision 18:** no rule; a note in
  [Boundary-Safe Values](../spec/10-modules.md#boundary-safe-values) points
  to `report_of`.
- **Decision 19:** [Generic Function Values](../spec/07-functions.md#generic-function-values)
  (`r[fn.type.generic.argument]`) and
  [Variant Constructors As Function Values](../spec/08-data-and-enums.md#variant-constructors-as-function-values)
  (`r[data.enum.fn-value.generic-argument]`) solve a generic value's type
  arguments with the call's; an unsolved one is
  `unresolved-generic-placeholder`.
- **Decision 20:** [Unsupported Aggregate Extensions](../spec/08-data-and-enums.md#unsupported-aggregate-extensions)
  lists non-exhaustive enums (`r[data.unsupported.non-exhaustive]`).

The prototype implements decision 19 and declares `ExitStatus`; test-block
`?` and entry-point chains are known failures (`EC-14`, `EC-17` in
`test/portable/KNOWN_FAILURES.tsv`).

## Current Design

The whole error design as decided on 2026-09-27, in one place. When a
decision changes it, update this section in the same change. The example
parses with the [reference parser](../spec/reference-parser/index.ts)
except the lines marked hypothetical, which need decision 12's grammar
extension (annotations on enum payload parameters).

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
    Yaml(@from error: YamlError)                        # hypothetical syntax: decision 12
    @error("`utils` is not configured correctly.")
    Utils(@source error: RuleSerializeError)            # hypothetical syntax: decision 12
    @error("`rule` is not configured correctly.")
    Rule(@from error: RuleSerializeError)               # hypothetical syntax: decision 12
    @error("Undefined meta var `$var` used in `$context`.")
    UndefinedMetaVar(var: string, context: string)

@error
pub enum LoadError:
    @error("$path:$line: invalid rule")
    Parse(path: string, line: i64, @source error: SyntaxError)   # hypothetical syntax: decision 12
    @error("cannot read $path")
    Read(path: string, @source error: FsError?)                  # hypothetical syntax: decision 12
    @error(transparent)
    Rules(@from error: RuleCoreError)                            # hypothetical syntax: decision 12

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

1. **Domain errors** are one enum per domain in `std` (`FsError`,
   `HttpError`), each implementing `Error`
   ([STDLIB decision 6](STDLIB.md#owner-decisions)).
2. **`@error`** (decision 10) is a compiler intrinsic, Rust's `thiserror`
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
3. **`?`** ([05 Propagation](../spec/05-expressions.md#propagation)):
   assignability by one rule (including construction of the erased
   `Error`), otherwise one call of the target's `From[E]`; never both, never
   chained; otherwise `invalid-result-propagation`. Only `?` converts;
   `return .Err(e)` uses assignability.
4. **`From[T]`** (`std.convert`) is pure (empty row, not suspending) and may
   panic ([09 Conversion Trait](../spec/09-traits.md#conversion-trait)).
5. **The erased error** is the dynamic trait value `Error`
   (`Error < Display + Inspectable`); `Result[T, Error]` holds any error.
   Recovery: `error.downcast[T]()`, `error.find[T]()` over the chain;
   `chain`, `root_cause`, `Context` and `.context(...)` are `std.error` API
   ([09 Error Trait](../spec/09-traits.md#error-trait),
   [STDLIB](STDLIB.md#stderror)).
6. **Mapping by hand:** a one-payload variant constructor is a function
   value, so `result.map_err(LoadError.Rules)` works (decision 7).
7. **Boundaries:** an erased `Error` never crosses a registered boundary or
   enters a durable history; code converts it to a domain enum or a report
   value first (decision 6).
8. **Entry point:** `pub fn main() -> Result[void, E]` with `E < Display`
   exits with status 1 on `.Err`, or with `error.status()` when `E`
   implements `std.process.ExitStatus` (decision 17); when `E` implements
   `Error` (including the erased `Error`) the runtime prints the message and
   then each cause as `caused by: ...`, otherwise `Display.to_string`
   (decision 13; [10 Entry Results](../spec/10-modules.md#entry-results)).
9. **Tests:** `?` works in a `test` block as if the block returned
   `Result[void, Error]`, and also accepts any `E < Display`, wrapped in a
   std message error; an `.Err` fails the test and prints the chain
   (decisions 14 and 16;
   [05 Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks)).
10. **Context and reports** (decision 15, `std.error` API):
    `result.context("loading rules")` wraps any `E < Error` into the erased
    `Error` as a std `Context { message, cause }`; `ErrorReport { message,
    causes }` with `report_of(error)` is the boundary-safe snapshot.
11. **Generic error types:** a generated impl for `enum AppError[E]` gets
    `E < Error` for a `@from`/`@source` member of type `E` and `E < Display`
    when a message interpolates it, per used parameter (typed derivation
    M12's rule).
12. **Not in hd, deliberately:** anonymous error unions (decision 9), a
   mapping clause on `?` (decision 8), chained conversions (decision 5),
   impl-family derivation outside `@error`.

## Still To Do

Not decided, and deliberately not specified:

- **`@error`** (decision 10) and decision 12 (payload-parameter
  annotations) are decided but not yet in the specification (02, 08, 09).
- **The std message error** of decision 16 has no name yet; it is
  library API for [STDLIB](STDLIB.md#stderror).
- **Open follow-ups** from decision 17 (out-of-range or zero exit statuses,
  and `ExitStatus` on an erased `Error`) are owner questions in
  [Open Issues](OPEN_ISSUES.md#error-entry-point-follow-ups).
- **`Console.write_line!` taking `mut self`,** raised by the recording
  `BufferConsole` in [STDLIB](STDLIB.md#stdconsole), is tracked with
  [Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers), not here.
- **Error-chain helpers** (`cause`, `Context`, `.context`, `chain`, a chain
  printer) are standard-library API for [STDLIB](STDLIB.md#stderror), not
  specification text.

## Contents

1. [The Problem](#the-problem)
2. [What The Specification Says Today](#what-the-specification-says-today)
3. [Survey](#survey)
4. [Constraints From hd](#constraints-from-hd)
5. [Candidate A: A Conversion Trait That `?` Calls](#candidate-a-a-conversion-trait-that--calls)
6. [Candidate B: One Error Trait And An Erased Error](#candidate-b-one-error-trait-and-an-erased-error)
7. [Candidate C: Anonymous Error Unions](#candidate-c-anonymous-error-unions)
8. [Candidate D: Explicit Mapping At The Site](#candidate-d-explicit-mapping-at-the-site)
9. [Candidate E: A, B, And D1 Together](#candidate-e-a-b-and-d1-together)
10. [Comparison](#comparison)
11. [Recommendation](#recommendation)
12. [Questions For The Owner](#questions-for-the-owner)

## The Problem

A function reads a URL from a file and fetches it. Each standard call returns
its own domain error:

```text
fn read_config(path: string) -> Result[string, FsError]:
    .Err(FsError.NotFound(path))

fn fetch(url: string) -> Result[string, HttpError]:
    .Err(HttpError.Timeout)
```

The caller wants to write this, and cannot:

```text
enum SyncError:
    Fs(error: FsError)
    Http(error: HttpError)

fn sync_bad(path: string) -> Result[string, SyncError]:
    url := read_config(path)?
    body := fetch(url)?
    .Ok(body)
```

Both blocks parse. The prototype compiler rejects `sync_bad` with
`invalid-result-propagation` at the first `?`: `FsError` is not the
function's error type, and nothing converts it.

Today the author has three ways out, all by hand:

1. **Match and return at every call.** Four lines per call, and the
   interesting line (`read_config(path)`) drowns in plumbing:

   ```text
   fn sync(path: string) -> Result[string, SyncError]:
       url := match read_config(path):
           .Ok(text) => text
           .Err(e) => return .Err(SyncError.Fs(e))
       body := match fetch(url):
           .Ok(b) => b
           .Err(e) => return .Err(SyncError.Http(e))
       .Ok(body)
   ```

   Parses. (The prototype also rejects this block, because it types the
   `return` arm as `void` instead of `never`; that is a prototype bug, not a
   language rule. Fixed 2026-09-26.)

2. **`map_err` with a closure.** Not in the normative method table yet;
   [STDLIB owner decision 8](STDLIB.md#owner-decisions) lets `std` add it as
   an inherent method. A payload-bearing variant constructor is not a
   first-class function
   ([Enum Declarations](../spec/08-data-and-enums.md#enum-declarations)), so
   each site needs a closure:
   `read_config(path).map_err(fn(e): SyncError.Fs(e))?`.

3. **Return an erased error.** Declare `Result[string, Error]` with a
   standard `Error` trait. Whether `?` accepts this today depends on how the
   specification's word "accepts" is read (next section).

The pain grows with the program. Every application that combines files,
network, and parsing repeats the same wrapping at every call. Agents will
write it correctly but verbosely, and reviewers will skim past the one
mapping that is wrong.

## What The Specification Says Today

[Result Types](../spec/04-type-system.md#result-types):

> Postfix `?` on `Result[T, E]` either produces the success value or
> immediately returns the error from the nearest function. The enclosing
> function must return a `Result[U, F]` whose error type accepts the
> propagated error.

[Propagation](../spec/05-expressions.md#propagation) says `.Err(error)`
"immediately returns a compatible `.Err`". Neither text defines "accepts" or
"compatible". Three facts pin down what is and is not decided:

- **No conversion call exists.** Nothing in the specification calls user code
  at `?`.
- **The natural reading of "accepts" is assignability.**
  [Assignability And Coercion](../spec/04-type-system.md#assignability-and-coercion)
  is the only defined notion of one type accepting a value of another. Under
  that reading `?` already performs one implicit step: numeric widening
  (`i32` error into an `i64` error type), permission weakening, and
  construction of a dynamic trait value (rule 6). So an `FsError` that
  implements a trait `Error` already propagates into `Result[U, Error]`.
- **The prototype reads it as exact match.** It rejects `?` from
  `Result[string, FsError]` into `Result[string, Error]` even when
  `FsError` implements `Error`. No conformance fixture decides between the
  two readings. [Question 1](#1-what-does-accepts-mean-for--today) asks
  the owner to settle it.

Related rules that any design must respect:

- **TQ-14: assignability stays single-step** ([audit/types/QUESTIONS.md](../audit/types/QUESTIONS.md)).
  A `User` passed to a `Display?` parameter needs an explicit conversion.
- **The entry point's error needs `Display`.** `main` may return
  `Result[void, E]` with `E < Display`; the host prints the error and exits
  with status 1 ([Executable Entry Point](../spec/10-modules.md#executable-entry-point)).
- **Dynamic trait values are not boundary-safe** ([Wasm Boundary](../spec/10-modules.md#wasm-boundary)).
  A registered tool or workflow cannot return `Result[T, Error]` with a
  dynamic `Error`. Durable histories reuse boundary-safe types
  ([Durable Replay decision 7](DURABLE_REPLAY.md)), so the same limit applies
  there.
- **No trait-value downcasts yet** (at the time of this survey).
  [Runtime Type Identity](../spec/09-traits.md#runtime-type-identity) now
  specifies `Inspectable` with the default method `downcast[T]()`, and the
  standard error trait extends `Inspectable`.
- **Coherence.** An impl may be written by the owner of the trait, of the
  target's constructor, or of a trait argument's constructor (TQ-2). Overlap
  is decided from impl heads alone; bounds never prove two impls disjoint
  (TQ-1, TQ-28). There are no blanket impls: `impl[T] X for T` is a
  `bare-parameter-impl-target` error. A trait value type is never an impl
  target (`trait-value-impl-target`).

## Survey

### Summary Table

| Language | At the propagation site | Typed or erased | Two error types in one function | Context and chains | Recovering the concrete error |
| --- | --- | --- | --- | --- | --- |
| Rust (std) | `?` calls `From::from` on the error | Typed; erased with `Box<dyn Error>` | One `From` impl per source type into the caller's enum, or box both | `Error::source()` chain | `downcast_ref::<T>()` on `dyn Error + 'static` |
| Rust (thiserror, anyhow) | Same `?`; `#[from]` generates the `From` impl; `.context("...")` wraps | thiserror typed for libraries, anyhow erased for applications | thiserror: `#[from]` variant per source; anyhow: everything converts | anyhow stores a context chain and prints it | `anyhow::Error::downcast_ref`, walks the chain |
| Go | Explicit `if err != nil { return fmt.Errorf("load: %w", err) }` | Erased `error` interface | Both are `error` | `%w` wrapping, `errors.Join` | `errors.Is` (identity), `errors.As` (type), both walk the chain |
| Swift | `try` propagates; typed throws `throws(E)` since SE-0413 | `any Error` by default, typed opt-in | No implicit conversion between typed errors; a `do` body throwing two types infers `any Error` | No standard chain | `catch let e as FsError` (dynamic cast) |
| Kotlin | Exceptions propagate implicitly; `Result<T>` fixes the error to `Throwable` | Erased, unchecked | Both are exceptions | `cause` chain on `Throwable` | `catch (e: IOException)`, `is` checks |
| Zig | `try x` is `x catch \|e\| return e` | Typed error sets, inferred with `!T` | Error sets merge automatically (`A \|\| B`); a subset coerces to a superset | Error return traces in debug builds; errors carry no payload | `switch` on the error tag |
| OCaml | Polymorphic variants with `let*` or `Result.bind` | Typed, structural | `[> \`Fs of ... \| \`Http of ...]` unify automatically | None built in | Pattern match on tags |
| Roc | `?` / `try` on `Result` | Typed, structural tag unions | Tag unions accumulate: `[FileErr ..., HttpErr ...]` is inferred | None built in | Pattern match on tags |
| Koka | Exceptions are the `exn` effect; custom effects for typed errors | Effect-typed; `exn` itself is erased | Effect rows union automatically | Via handlers | Handlers |
| MoonBit | `f()` in a `raise` function propagates; `try`/`catch`, `try?` to `Result` | `raise E` typed, bare `raise` means `Error` | Every `suberror` converts to `Error`; `E1` into `E2` needs a manual `catch` | None built in | `catch` with patterns on the `Error` value |
| Scala ZIO | `ZIO[R, E, A]`; `flatMap` widens `E` | Typed; Scala 3 infers `E1 \| E2` unions | Union types inferred at each `flatMap` | `Cause` tree | Pattern match; `refineOrDie`, `mapError` |

### Takeaways

1. **Every typed-error language grew two tiers.** Rust has thiserror for
   libraries and anyhow for applications. Swift has typed throws next to the
   default `any Error`. MoonBit has `raise E` next to bare `raise`. Precise
   errors matter where a caller branches on them; an erased error is fine
   where the only consumer prints it.
2. **Swift chose no implicit conversion and paid for it.** SE-0413 recommends
   typed throws only for module-internal code, generic pass-through, and
   constrained targets, because nothing converts `E1` into `E2`. MoonBit makes
   the same trade: typed errors compose only into `Error`. The lesson is that
   typed errors without a conversion path push users toward the erased form.
3. **Rust's `From` at `?` is the most widely copied answer**, and it is the
   one agents write most fluently. Its costs are a hidden call at `?`, one
   impl per pair, and no way to send one source type into two different
   variants (users fall back to `map_err`).
4. **Structural unions compose for free but read badly.** Zig error sets,
   OCaml polymorphic variants, Roc tag unions, and Scala 3 unions need no
   impls. Inferred sets grow silently, and OCaml and Roc error messages over
   large inferred unions are hard to read. Zig keeps them readable only by
   giving errors no payload.
5. **Context matters more than type for diagnosis.** Go's `%w` and anyhow's
   `.context(...)` exist because "file not found" alone does not say which
   step failed. A good design makes adding a sentence cheap and keeps the
   cause reachable.
6. **Recovery needs runtime type identity.** `errors.As`, `downcast_ref`, and
   Swift's `as` casts all ask the runtime for the concrete type. hd's
   `Inspectable` direction matches Rust's `'static` plus `downcast_ref`.
7. **For agents and reviewers**, the questions are locality and uniqueness.
   A reader of `x?` should be able to find the conversion in one known place,
   and there should be exactly one. Rust meets this through coherence;
   Zig and Roc meet it by making the conversion structural; Go meets it by
   writing everything at the site.

## Constraints From hd

- **Nominal types.** Data and enums are nominal; only tuples and function
  types are structural. A union type former would be the first structural sum.
- **Single-step implicit conversion (TQ-14).** Any conversion at `?` should be
  one step, never a chain.
- **Explicit public signatures.** Public result types are always written, so
  inferred error sets could only help private functions.
- **Least-common-type never falls back to `Any` or a trait value.** Inference
  must not invent an erased error type either.
- **Requirement rows.** A conversion that could use a requirement or suspend
  would hide an effect at `?`. Any conversion must have an empty row and no
  `!`.
- **Boundary safety.** Registered functions and durable histories accept only
  boundary-safe values, so the precise enums must remain usable end to end.
- **Replay.** Guest code between host calls is deterministic; a pure
  conversion changes nothing that replay records.
- **Prelude is fixed** (STDLIB decision 7). New error names live in
  `std.error` and are imported.
- **No derivation beyond comparison and hash for now** (TQ-13). A
  thiserror-style `#[from]` cannot be generated yet.

## Candidate A: A Conversion Trait That `?` Calls

Rust's `From`, narrowed to errors.

### Syntax

No new syntax. `std.error` declares a trait, and `?` consults it:

```text
pub trait FromError[E]:
    fn from_error(error: E) -> Self

pub enum SyncError:
    Fs(error: FsError)
    Http(error: HttpError)
    BadStatus(status: u16)

impl FromError[FsError] for SyncError:
    fn from_error(error: FsError) -> SyncError: SyncError.Fs(error)

impl FromError[HttpError] for SyncError:
    fn from_error(error: HttpError) -> SyncError: SyncError.Http(error)

pub fn sync!(path: Path) -> Result[Response, SyncError] $ FsRead, Http:
    files, http := $.use(FsRead, Http)
    url := files.read_text!(path)?
    response := http.send!(Request::get(url.trim()))?
    if response.status != 200:
        return .Err(SyncError.BadStatus(response.status))
    .Ok(response)
```

Parses. Type-correct under A.

### Typing rule

Let the operand have type `Result[T, E]` and let the nearest enclosing
function or closure have result type `Result[U, F]`. On `.Err(error)`:

1. If `E` is assignable to `F` by one rule of
   [Assignability And Coercion](../spec/04-type-system.md#assignability-and-coercion),
   `?` returns `.Err` with the assigned value. This covers identity, widening,
   weakening, and construction of a dynamic `Error` value.
2. Otherwise, if `F` implements `FromError[E']`, where `E'` is `E` with an
   outer `mut` removed, `?` returns `.Err(FromError[E']::from_error(error))`
   with `Self = F`. For a generic `F` with the bound `F < FromError[E']`, the
   call goes through the bound's dictionary (TQ-9).
3. Otherwise it is an `invalid-result-propagation` error. The message names
   `E` and `F` and suggests `impl FromError[E] for F` or `.map_err(...)`.

The two steps never combine: `?` does not convert `FsError` to `SyncError`
and then widen, or construct a trait value and then convert. This keeps TQ-14's
single step. The conversion runs before any `defer` cleanup, like the rest of
the propagated value ([Deferred Cleanup](../spec/06-control-flow.md#deferred-cleanup)). A closure whose
result type is inferred gets no conversion: `?` contributes `E` unchanged, and
a conversion needs a written or expected result type.

`from_error` is an associated function, so `FromError` is not dynamically
safe. That is fine: `?` always knows `F` statically. The trait signature fixes
an empty row and no `!`, so a conversion cannot use a requirement, suspend, or
make a host call. It can still panic, as any code can.

The conversion applies only at `?`. `return .Err(fs_error)` in a function
returning `Result[U, SyncError]` is still a type error; write
`.Err(SyncError.Fs(fs_error))`.

### Coherence and orphan rule

- `impl FromError[FsError] for SyncError` may live in the package that owns
  `SyncError` (the target), in `std` (the trait), or in the package that owns
  `FsError` (a trait argument, TQ-2). If two packages write the same pair,
  the resolved graph is rejected, as for any duplicate impl. TQ-17 puts it in
  the module that declares `SyncError` or `FsError`, so a reader looks in two
  places at most.
- Conversions from different sources never overlap: `FromError[FsError]` and
  `FromError[HttpError]` differ in the trait argument (TQ-28).
- A generic conversion is allowed and useful:

  ```text
  enum ResourceError[E]:
      Operation(error: E)
      Disposed

  impl[E] FromError[E] for ResourceError[E]:
      fn from_error(error: E) -> ResourceError[E]: ResourceError.Operation(error)
  ```

  Parses. It does not overlap an impl of `FromError[ResourceError[X]]` for
  `ResourceError[X]`, because unification fails the occurs check. A generic
  `impl[E < Error] FromError[E] for Report` does overlap every specific
  `FromError[X] for Report`, since bounds are ignored (TQ-1). A type
  chooses either one generic conversion or a set of specific ones.
- No reflexive impl is needed, and none could be written (`impl[T]
  FromError[T] for T` is a `bare-parameter-impl-target` error). Step 1
  handles identity.
- `impl FromError[FsError] for Error` is a `trait-value-impl-target` error.
  Conversion into the erased error happens only through step 1.

### Generics

A pass-through combinator such as `retry![T, E]` returns `Result[T, E]` and
propagates `E` unchanged by step 1. A generic function can ask for a
conversion through a bound:

```text
fn load[E < FromError[FsError]](path: Path) -> Result[string, E] $ FsRead:
    text := $.use(FsRead).read_text!(path)?
    .Ok(text)
```

Parses. It needs static calls through a bound (TQ-9, now in
[Associated Function Calls](../spec/09-traits.md#associated-function-calls)).

### `fn!`, replay, and boundaries

`?` behaves identically in `fn!` bodies; the conversion is an ordinary
non-suspending call. Replay is unaffected: the host result (an `FsError`
decoded from the boundary) is recorded, and the conversion re-runs
deterministically. `SyncError` is boundary-safe when its payloads are, so it
can be the error of a registered tool or a durable workflow.

### Display and messages

Each application error still needs a hand-written `Display` (TQ-13 rules out
derived `Display`) and, to join the chain, an `Error` impl with `cause`:

```text
impl Display for SyncError:
    fn to_string(self) -> string:
        match self:
            SyncError.Fs(inner) => "file: " + inner.to_string()
            SyncError.Http(inner) => "http: " + inner.to_string()
            SyncError.BadStatus(status) => "status $status"

impl Error for SyncError:
    fn cause(self) -> Error?:
        match self:
            SyncError.Fs(inner) => .Some(inner)
            SyncError.Http(inner) => .Some(inner)
            SyncError.BadStatus(_) => .None
```

Parses. Note `.Some(inner)`: TQ-14 forbids the two-step `FsError` to `Error`
to `Error?`, so the wrap is explicit.

### Cost

One static call per propagated error, usually one enum allocation for the
wrapping variant. Nothing on the success path. Two lines per (source, target)
pair.

### For agents and reviewers

Rust-trained agents write this pattern without prompting. The hidden call is
unique per pair and sits next to the target type. The weakness is the one
Rust has: one source type maps to one variant. A function that reads both a
config file and a data file and wants `Config(FsError)` and `Data(FsError)`
must use `map_err` for at least one of them.

## Candidate B: One Error Trait And An Erased Error

Go's `error`, Swift's `any Error`, anyhow.

### Syntax

No new syntax. `std.error` declares the trait the STDLIB draft already
sketches, plus a context wrapper and a chain walker:

```text
pub trait Error < Display + Inspectable:
    fn cause(self) -> Error?:
        .None

pub data Context:
    pub message: string
    pub inner: Error

impl Display for Context:
    fn to_string(self) -> string:
        self.message

impl Error for Context:
    fn cause(self) -> Error?:
        .Some(self.inner)

impl[T, E < Error] Result[T, E]:
    pub fn context(self, message: string) -> Result[T, Error]:
        match self:
            .Ok(value) => .Ok(value)
            .Err(error) => .Err(Context { message: message, inner: error })

pub fn chain(error: Error) -> List[Error]:
    let found: mut List[Error] = [error]
    let current: Error? = error.cause()
    while true:
        match current:
            .Some(next) =>
                found.push(next)
                current = next.cause()
            .None => break
    found
```

Parses. `Inspectable` waits on
[Runtime Type Identity](OPEN_ISSUES.md#runtime-type-identity-and-reified);
until then `Error` has `Display` and `cause` only. The inherent
`impl ... Result[T, E]` relies on STDLIB decision 8, and `push` stands for
the future `List` method.

Application code then returns the erased error:

```text
pub fn report!() -> Result[string, Error] $ FsRead, Http:
    files, http := $.use(FsRead, Http)
    url := files.read_text!(Path::parse("endpoint.txt")).context("reading endpoint")?
    response := http.send!(Request::get(url))?
    .Ok("status ${response.status}")

pub fn main!() -> Result[void, Error] $ FsRead, Http, Console:
    match report!():
        .Ok(line) => println(line)
        .Err(error) =>
            for part in chain(error):
                match part.downcast[FsError]():
                    .Some(FsError.NotFound(path)) => println("missing ${path}")
                    _ => println(part.to_string())
            return .Err(error)
    .Ok()
```

Parses. `downcast` is the compiler-provided method TQ-22 decided.

### Typing rule

None new, if [question 1](#1-what-does-accepts-mean-for--today) confirms
that `?` accepts any assignable error type. `FsError` implements `Error`, so
rule 6 of assignability constructs the dynamic value. Otherwise B needs a
one-line rule: `?` may construct a dynamic trait value of `F` from `E`.

`main!` returning `Result[void, Error]` also needs the dynamic type `Error`
to satisfy the entry bound `E < Display`. The specification does not say
whether a dynamic trait value type satisfies a bound on its own trait or its
supertraits ([question 5](#5-does-a-dynamic-trait-value-satisfy-a-bound-on-its-own-trait)).
The same question decides whether `context` can be called on a
`Result[T, Error]`.

### Coherence

Every domain error writes `impl Error for FsError`. Nothing else is needed:
there are no pairwise impls, and dynamic trait values cannot be impl targets,
so no package can add behavior to `Error` itself.

### Generics

A generic `E < Error` converts to the dynamic value through its bound's
dictionary. Erasing needs the full runtime type for `Inspectable`
([Runtime Type Identity item 11](OPEN_ISSUES.md#runtime-type-identity-and-reified)),
which the dictionary carries.

### `fn!`, replay, and boundaries

Nothing changes for `fn!`. A dynamic `Error` is not boundary-safe, so it can
never be a registered function's error or part of a durable history. That is
a real limit: the [Durable Replay question 11](DURABLE_REPLAY.md) sketch
`@tool fn get_user!(id: UserId) -> Result[User, Error]` would be rejected.
Code at a boundary converts explicitly, for example to a boundary-safe
snapshot:

```text
pub data ErrorReport:
    pub message: string
    pub causes: List[string]

pub fn report_of(error: Error) -> ErrorReport:
    causes := chain(error).map(fn(part): part.to_string())
    ErrorReport { message: error.to_string(), causes: causes }
```

Parses.

### Display and messages

The chain gives Go- and anyhow-style messages for free: `std.error` can print
`reading endpoint: not found: endpoint.txt` by joining `chain(error)`.

### Cost

One trait-value construction per propagated error (a record holding the
reference and a method table), plus one record per `context`. Walking the
chain is linear in its depth; each downcast compares runtime type objects.
Nothing on the success path.

### For agents and reviewers

Very little to write. What is lost is exhaustiveness: a reader of
`Result[string, Error]` cannot tell which failures are possible, and a
`match` on the error needs downcasts rather than patterns. It is the right
tool where the only consumer prints the error, and the wrong one for library
APIs and boundaries.

## Candidate C: Anonymous Error Unions

Zig error sets, Roc tag unions, Scala 3 unions.

### Syntax

A new type former in the error position, and new type patterns:

```text
fn sync!(path: Path) -> Result[Response, FsError | HttpError] $ FsRead, Http:
    url := $.use(FsRead).read_text!(path)?
    response := $.use(Http).send!(Request::get(url))?
    .Ok(response)
```

Hypothetical: the parser rejects `|` in a type.

```text
fn describe(error: SyncFailure) -> string:
    match error:
        FsError.NotFound(path) => "missing ${path}"
        FsError(other) => other.to_string()
        HttpError(other) => other.to_string()
```

This parses, but only as a call pattern on a variant named `FsError`; as a
type pattern it is hypothetical. Real type patterns would need a new pattern
form, and `is` is already identity comparison.

### Typing rule

`A | B` is a new structural type. `A` is assignable to `A | B` (a new
assignability rule, and not representation-preserving: the value needs a
runtime tag). `?` would inject `E` into a union `F` that contains it.
Optionally, a private function's error union is inferred from its body.

### Problems in hd

- **A second structural sum.** Every other sum in hd is a nominal enum. Union
  types bring subtyping between unions, normalization (`A | A`, order), and
  the classic generic problem: in `Result[T, E | HttpError]`, `E` may be
  `HttpError`, and arms overlap.
- **Runtime type identity.** Matching a member needs the runtime type object
  that `Inspectable` would supply, for every member.
- **Inference conflicts.** Public result types are written anyway, and
  least-common-type inference refuses to invent erased types; an inferred
  union would be the first exception.
- **Boundary encoding.** A union has no boundary encoding; one would have to
  be specified in the component model.
- **Coherence** is not needed, which is the attraction.

### Cost

A tag per value and a type test per arm. A large type-system addition.

### For agents and reviewers

Written unions are honest and compose with no boilerplate. Inferred unions
are the opposite of reviewable: the error type of a private helper changes
when a line deep inside it changes.

## Candidate D: Explicit Mapping At The Site

Go's style: say the wrapping on the line.

### D1: a library `map_err`

No language change. `std` adds an inherent method on `Result`, with a
row parameter so the callback may use requirements:

```text
pub enum ConfigError:
    Missing(error: FsError)
    Unreadable(error: FsError)
    Network(error: HttpError)

impl[T, E] Result[T, E]:
    pub fn map_err[F, R](self, transform: fn(E) -> F $ R) -> Result[T, F] $ R:
        match self:
            .Ok(value) => .Ok(value)
            .Err(error) => .Err(transform(error))

fn load!(path: Path, backup: Path) -> Result[string, ConfigError] $ FsRead:
    files := $.use(FsRead)
    primary := files.read_text!(path).map_err(fn(e): ConfigError.Missing(e))?
    extra := files.read_text!(backup).map_err(fn(e): ConfigError.Unreadable(e))?
    .Ok(primary + extra)
```

Parses. This is the only design here that sends one source type to two
variants, which `load!` needs.

Passing the constructor directly, `.map_err(ConfigError.Missing)?`, parses
but is an `unsaturated-enum-constructor` error today.
[Question 8](#8-may-a-single-payload-variant-constructor-stand-for-a-function)
asks whether to relax that under an expected function type.

### D2: a mapping clause on `?`

The task brief suggested `expr?(AppError.Fs)`. **That spelling is already
taken**: `?` is a postfix operator at call precedence, so `f()?(x)` calls the
success value. This parses and is legal today:

```text
fn make_adder() -> Result[fn(i32) -> i32, string]:
    .Ok(fn(x: i32) -> i32: x + 1)

fn use_adder() -> Result[i32, string]:
    three := make_adder()?(2)
    .Ok(three)
```

`?[...]` is indexing the success value and `??` is two unwraps of a `T??`, so
neither is free. A free spelling is `? else`:

```text
fn sync!(path: Path) -> Result[Response, SyncError] $ FsRead, Http:
    url := $.use(FsRead).read_text!(path)? else SyncError.Fs
    response := $.use(Http).send!(Request::get(url))? else SyncError.Http
    .Ok(response)
```

Hypothetical: the parser rejects it. `else` in expression position is always
followed by `:` today, so `? else` followed by an expression is unambiguous.

Typing: in `operand? else mapper`, `mapper` is a single-payload variant
constructor or an expression of type `fn(E) -> F`; on `.Err(error)`, `?`
returns `.Err(mapper(error))`. No trait and no coherence question.

### Coherence, generics, `fn!`, boundaries, cost

None of these change: the mapping is an ordinary call written by the user.
Cost is the same as A. D1 costs a closure allocation per call unless the
compiler inlines it.

### For agents and reviewers

Every conversion is on the line, which is the most local form possible. The
price is noise at every call; in a function with ten calls to one domain, the
same mapping appears ten times, and a wrong one hides among the right ones.

## Candidate E: A, B, And D1 Together

Rust's settled model (`From` plus `Box<dyn Error>` plus `map_err`), adjusted
to hd's rules:

- **Libraries and boundary code** return a precise enum and write one
  `FromError` impl per source domain (A). `?` converts, the enum stays
  boundary-safe, and callers match exhaustively.
- **Applications and scripts** return `Result[T, Error]` (B). `?` converts
  any domain error through assignability, `.context("...")` adds a sentence,
  and `downcast` recovers a concrete error once `Inspectable` lands.
- **Site-specific mapping** uses `map_err` (D1) when one source type needs
  different variants at different calls.

The pieces do not interact: A's step 2 applies only when step 1
(assignability, which covers B) fails, and `Error` can never be a
`FromError` target.

## Comparison

| | A: `FromError` | B: erased `Error` | C: unions | D1: `map_err` | D2: `? else` | E: A + B + D1 |
| --- | --- | --- | --- | --- | --- | --- |
| Language change | `?` step 2 | none (if question 1 is yes) | new type former, patterns | none | new syntax | `?` step 2 |
| Boilerplate | one impl per pair | one `impl Error` per domain | none | per call | per call | per pair or none |
| Exhaustive match | yes | no | yes | yes | yes | where it matters |
| One source to two variants | no | n/a | no | yes | yes | yes (D1) |
| Boundary-safe | yes | no | needs encoding | yes | yes | yes where needed |
| Conversion visible at site | no (unique, next to target) | no (always the same) | no | yes | yes | mixed |
| Depends on open issues | TQ-9 for generic targets | `Inspectable`, question 5 | runtime identity | none | none | as A and B |
| Survey precedent | Rust | Go, Swift, anyhow, MoonBit | Zig, Roc, OCaml, Scala 3 | Rust, Scala | none | Rust ecosystem |

## Recommendation

**E: a narrow `FromError` trait that `?` consults after assignability, the
erased `std.error.Error` for applications, and `map_err` and `context` as
library methods.** Reject C, and defer D2.

Reasons:

1. It keeps the owner's one-enum-per-domain decision useful. Without a
   conversion path, typed errors push users toward the erased form, as Swift
   and MoonBit show.
2. It keeps TQ-14: `?` performs exactly one step, either an assignability rule
   or one `FromError` call.
3. It needs no new syntax and one new typing rule. Coherence, the orphan rule,
   and overlap apply unchanged.
4. The hidden call is pure (no row, no `!`), unique per pair, and located next
   to the target type by TQ-17.
5. Boundaries and durable histories keep precise, boundary-safe enums; the
   erased error stays inside the program.
6. It is the model agents already write most reliably.

Staging:

1. **Now, without language change:** confirm question 1, then ship
   `std.error.Error` with `Display` and `cause`, `Context`, `chain`,
   `Result.map_err`, and `Result.context`. B and D1 work from day one.
2. **Specification change:** add step 2 to
   [Result Types](../spec/04-type-system.md#result-types) and
   [Propagation](../spec/05-expressions.md#propagation), with fixtures for:
   conversion accepted; no impl (`invalid-result-propagation`); assignability
   preferred over an impl; closure with inferred result gets no conversion;
   `return .Err(e)` not converted.
3. **With Runtime Type Identity:** `Error < Display + Inspectable`,
   `downcast`, and a chain search (`std.error.find[reified T]`, like Go's
   `errors.As`).
4. **With typed derivation:** revisit generating the two-line `FromError`
   impls from an enum declaration (thiserror's `#[from]`).

## Questions For The Owner

### 1. What does "accepts" mean for `?` today?

The specification says the enclosing error type "accepts the propagated
error". The prototype requires an exact match; no fixture decides.

- **A.** Assignability: identity, widening, weakening, and dynamic trait
  value construction all apply.
- **B.** Exact match only (the prototype's reading).
- **C.** Exact match or permission weakening only.

**Recommendation: A.** It is the only defined notion of "accepts", it
matches `return` and argument passing, and it makes the erased `Error` work
with no new rule.

```text
fn sync(path: string) -> Result[string, Error]:
    url := read_config(path)?    # FsError to Error by assignability rule 6
    .Ok(url)
```

### 2. Does `?` call a conversion, and through which trait?

- **A.** A narrow `std.error.FromError[E]` with
  `fn from_error(error: E) -> Self`, consulted only by `?`, only after
  assignability fails.
- **B.** A general `std.convert.From[T]`, also usable by other code, which
  `?` consults.
- **C.** No conversion at `?`; only assignability and `map_err`.

**Recommendation: A.** A narrow name tells readers that it runs at `?` and
nowhere else, and it does not invite a general implicit conversion that
TQ-14 rejected. B could come later as a separate trait. C leaves every
multi-domain function to hand-written mapping.

```text
impl FromError[FsError] for SyncError:
    fn from_error(error: FsError) -> SyncError: SyncError.Fs(error)
```

### 3. Is the erased error the dynamic `Error` value or a nominal `AnyError`?

- **A.** The dynamic trait value `Error` itself: `Result[T, Error]`.
- **B.** A nominal `std.error.AnyError` data type wrapping an `Error`, so it
  can carry extra fields and gain trait impls (a trait value cannot be an
  impl target). It needs `impl[E < Error] FromError[E] for AnyError`.

**Recommendation: A.** No new type, no generic impl that overlaps any user
impl for `AnyError`, and `downcast` already works on `Inspectable` values.

```text
pub fn main!() -> Result[void, Error] $ FsRead, Console:
    text := $.use(FsRead).read_text!(Path::parse("a.txt"))?
    println(text)
    .Ok()
```

### 4. Does `std` ship `context` and a chain walker?

- **A.** Yes: `Result.context(message) -> Result[T, Error]`, `Context`,
  `chain`, and a printer that joins the chain.
- **B.** Only `map_err`; users build their own wrappers.

**Recommendation: A.** The survey's strongest lesson is that "which step
failed" is the missing fact in most error reports, and one standard wrapper
keeps chains uniform for tools.

```text
url := files.read_text!(path).context("reading endpoint")?
```

### 5. Does a dynamic trait value satisfy a bound on its own trait?

The specification does not say whether the type `Display` (a dynamic value)
satisfies `T < Display`, or whether `Error` satisfies `E < Display` through
its supertrait. `main` returning `Result[void, Error]` needs this, and so
does calling `context` on a `Result[T, Error]`.

- **A.** Yes, for the value's own trait and all its supertraits.
- **B.** No; callers convert with `.to_string()` or unwrap first.

**Recommendation: A.** Dispatch through the value's table gives each method a
body, and dynamic safety already guarantees the trait has no associated
functions a dictionary would need. This belongs to area 2 generally, not only
to errors.

```text
pub fn main() -> Result[void, Error]:
    .Err(Context { message: "no input", inner: FsError.Other("empty") })
```

### 6. How does an erased error cross a registered boundary?

- **A.** It does not. Registered functions return boundary-safe enums; code
  converts explicitly, for example with `report_of(error) -> ErrorReport`.
- **B.** The adapter converts a dynamic `Error` to `ErrorReport`
  automatically.
- **C.** Make dynamic `Error` boundary-safe by encoding its message chain.

**Recommendation: A.** B and C lose the concrete type silently at the edge
where precision matters most. The Durable Replay sketch
`@tool fn get_user!(...) -> Result[User, Error]` should change to a domain
enum.

```text
@tool
fn get_user!(id: UserId) -> Result[User, DbError] $ Database:
    $.use(Database).load_user!(id)
```

### 7. Where may a `FromError` impl live?

TQ-2 lets the owner of `FsError` write `impl FromError[FsError] for
SyncError` as well as the owner of `SyncError`.

- **A.** Any owner under TQ-2, in the module TQ-17 names; duplicates are
  rejected at resolution as for any impl.
- **B.** A special rule for `FromError`: only the target's owner.

**Recommendation: A.** No special case. A library that writes conversions
into another library's error type is unusual but harmless, and a duplicate
already fails to resolve.

```text
# package acme_http owns HttpError; legal under TQ-2
impl FromError[HttpError] for SyncError:
    fn from_error(error: HttpError) -> SyncError: SyncError.Http(error)
```

### 8. May a single-payload variant constructor stand for a function?

`.map_err(ConfigError.Missing)` is an `unsaturated-enum-constructor` error
today.

- **A.** Keep the rule; write `fn(e): ConfigError.Missing(e)`.
- **B.** Allow a payload-bearing constructor where a function type is
  expected.

**Recommendation: A** for now. With A in place, `map_err` is the exception
rather than the rule, so the saving is small. Revisit if `map_err` sites are
common in real code.

```text
primary := files.read_text!(path).map_err(fn(e): ConfigError.Missing(e))?
```

### 9. Does `?` get a mapping clause?

- **A.** No new syntax; `map_err` covers site-specific mapping.
- **B.** Add `operand? else mapper`. (`operand?(mapper)` is impossible: it
  already calls the success value.)

**Recommendation: A.** One operator gains a second form for a case D1 covers
in about fifteen more characters.

```text
url := files.read_text!(path)? else SyncError.Fs    # hypothetical under B
```

### 10. Should hd add error unions?

- **A.** No. Errors stay nominal enums or the erased `Error`.
- **B.** Written unions `FsError | HttpError` in error positions.
- **C.** Written unions, inferred for private functions.

**Recommendation: A.** Unions add a structural sum type, type patterns, and a
boundary encoding. They need runtime type identity for every member. The
gain over A plus B is the saved `FromError` impls. Revisit only if hd adopts
union types in general.

```text
fn sync!(path: Path) -> Result[Response, FsError | HttpError] $ FsRead, Http:
    pass
```

Hypothetical under B and C.

### 11. Are conversions derived from the enum declaration?

- **A.** Not now; the two-line impls are written by hand (TQ-13).
- **B.** Add a compiler-known marker on a variant that generates the impl.

**Recommendation: A.** TQ-13 forbids ad-hoc derivation. When the typed
derivation protocol lands, a library derivation can generate these impls
like any other.

```text
impl FromError[HttpError] for SyncError:
    fn from_error(error: HttpError) -> SyncError: SyncError.Http(error)
```

## Sources

- Rust: [`?` and `From`](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html),
  [`std::error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html),
  [thiserror](https://docs.rs/thiserror), [anyhow](https://docs.rs/anyhow).
- Go: [Working with Errors in Go 1.13](https://go.dev/blog/go1.13-errors),
  [`errors.Join`](https://pkg.go.dev/errors#Join).
- Swift: [SE-0413 Typed throws](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0413-typed-throws.md).
- Kotlin: [Exceptions](https://kotlinlang.org/docs/exceptions.html),
  [`Result`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/-result/).
- Zig: [Errors](https://ziglang.org/documentation/master/#Errors).
- OCaml: [Polymorphic variants](https://ocaml.org/manual/latest/polyvariant.html).
- Roc: [Tag unions and `Result`](https://www.roc-lang.org/tutorial).
- Koka: [Effect handlers](https://koka-lang.github.io/koka/doc/book.html).
- MoonBit: [Error handling](https://docs.moonbitlang.com/en/latest/language/error-handling.html).
- Scala ZIO: [Typed errors](https://zio.dev/reference/error-management/).
