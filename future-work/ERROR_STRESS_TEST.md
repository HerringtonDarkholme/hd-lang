# Error Design Stress Test (V1, V2a, V2b)

Status: design review, 2026-09-27. Nothing here is accepted language
behavior. It changes no decision, no design record, no specification text,
and no prototype code. Every design choice below is a question for the
owner.

The design under test is [Error Conversion](ERROR_CONVERSION.md): the
[Owner Decisions](ERROR_CONVERSION.md#owner-decisions) 1-15, decision 10
with its refinements E1-E4 and reviews R6, R7, R10, and R12, and the
[Current Design](ERROR_CONVERSION.md#current-design) section. The library
surface is [STDLIB `std.error`](STDLIB.md#stderror) (`chain`, `find`,
`root_cause`, `Context`, `ErrorReport`). The language rules are
[05 Propagation](../spec/05-expressions.md#propagation),
[09 Conversion Trait](../spec/09-traits.md#conversion-trait) and
[Error Trait](../spec/09-traits.md#error-trait),
[10 Executable Entry Point](../spec/10-modules.md#executable-entry-point),
[08 Enum Declarations](../spec/08-data-and-enums.md#enum-declarations)
(shared enum data, one-payload constructors as function values), and
interpolation in [01](../spec/01-lexical-structure.md) and
[05 Literals](../spec/05-expressions.md#literals). The earlier error
findings are [Derivation Stress Test 2, case 1](DERIVATION_STRESS_TEST_2.md#1-error-enums-and-deriveerror);
their problem ids (R4, R6, R7, R10, R11, R12, R16, R18) are reused here.

## Contents

1. [The Three Variants](#the-three-variants)
2. [Method](#method)
3. [Summary](#summary)
4. [Cases](#cases) (1-14)
5. [Marker Scope](#marker-scope)
6. [Where Library Derivations Want The Same Access](#where-library-derivations-want-the-same-access)
7. [V1, V2a, And V2b Compared](#v1-v2a-and-v2b-compared)
8. [Problems, Ranked](#problems-ranked)
9. [Parse Log](#parse-log)

## The Three Variants

All three share everything outside `@derive(Error)`: `?` converts in one
step (assignability, else one `From` call), `From` is pure, the erased error
is the dynamic value `Error`, one-payload constructors are function values,
the entry point prints the chain (decision 13), `?` works in tests
(decision 14), and `std.error` has `Context`, `.context(...)`,
`ErrorReport`, and `report_of` (decision 15). All three also infer the same
`From` impls and the same cause.

- **Shared inference.** Automatic `From[P]` for a one-payload variant whose
  payload type `P` implements `Error` and is unique among such payloads;
  among variants that share `P`, only a `@from`-marked one gets it. A bare
  type-parameter payload is never promoted (R6). A common enum field
  without a default blocks promotion (R12). The cause is the member whose
  type implements `Error`, or is `E?` with `E < Error`; `@source` picks one
  of two.
- **V1 (the record).** `@message("... $member ...")` per variant or on a
  data type. Only this intrinsic marker sees the annotated variant's
  payload members, a data type's fields, and the enum's common fields (E1).
  `Display` is always generated (E2); a variant without `@message` displays
  as its name. `@transparent` on a one-payload variant forwards `Display`
  and makes `cause()` return the payload's cause (E3).
- **V2a.** No `@message`. `Display` is a hand-written `impl Display` with a
  `match` and ordinary interpolation. `@derive(Error)` generates only
  `impl Error` (with the inferred `cause()`) and the automatic `From`
  impls. Markers: `@from`, `@source`, and `@transparent`, which now affects
  only `cause()` (the payload's cause, as E3); the author forwards the
  message in the `Display` arm.
- **V2b.** As V2a without `@transparent`. A wrapper variant's cause is its
  payload. The standard chain printer (entry point, failing tests) and
  `report_of` skip a part whose message equals the previous part's.
  `chain`, `find`, and `root_cause` are unchanged: they see every part.

What the compiler generates, for `Rules(error: RuleCoreError)` under each:

| | V1 (`@transparent`) | V2a (`@transparent`) | V2b (no marker) |
| --- | --- | --- | --- |
| `Display` arm | generated: `error.to_string()` | hand-written by the author | hand-written by the author |
| `cause()` arm | `error.cause()` | `error.cause()` | `.Some(error)` |
| `From[RuleCoreError]` | generated (unique payload) | generated | generated |
| In `chain(outer)` | outer, then the payload's cause | same as V1 | outer, payload, payload's cause |
| Printed chain | inner message once | inner message once, if the arm forwards | inner message once (duplicate skipped) |

## Method

Each case translates the public error type of a real crate, as closely as
hd allows. The Rust shapes are approximations reconstructed from the
crates' public sources (field names and variant lists are close, not
copied); the report says which crate. Each case gives the Rust shape, the
hd code for V1 and for V2 (V2a and V2b differ only in the lines marked
`# V2a only`), what the compiler generates, how `?`, `find`, `chain`,
`root_cause`, and `report_of` behave, and what breaks.

Counting rules:

- **Lines** count the error types and their `Display`, `Error`, and `From`
  impls: non-blank lines that are not comments or `use` lines. Helper
  functions (quoting, position text), inherent methods (`kind()`,
  `classify()`), and usage code are not counted, in Rust or in hd. Rust is
  counted with one statement per line, not rustfmt's wrapping.
- **Markers** count the error attributes: in Rust `#[error]` (including
  `#[error(transparent)]`), `#[from]`, and `#[source]`; in hd `@message`,
  `@from`, `@source`, and `@transparent`. `#[derive(Error)]` and
  `@derive(Error)` are not counted.
- **Verdicts:** *works* (the variant expresses the crate's behavior with
  the derive or with ordinary hand-written impls, and `?`, `find`, the
  printed chain, and `report_of` behave as in Rust); *friction* (it works
  with a workaround, extra code, or a behavior difference a user would
  notice); *breaks* (the derive cannot express the shape, so every impl is
  hand-written, or a documented operation gives a different answer than the
  original).

Every `text` block was checked with the reference parser
([Parse Log](#parse-log)). Lines that use syntax no chapter specifies yet
end in `# hypothetical syntax`: today that is only `@source` on a payload
parameter (decision 12 is not yet in 02). Parsing checks syntax only;
nothing was type-checked, and the prototype implements none of this.

One assumption runs through cases 5 and 8: that the arguments of a
variant's `->` clause may name that variant's payload members, as in
`Unexpected(at: Span) -> WgslError(at)`. 08 does not say
([problem 9](#9-common-fields-per-value-data-and-decision-10s-two-outcomes)).

## Summary

Cells give verdict, markers, and lines. Rust is the original crate's
count for the same subset of variants; for naga it counts the `Error`
enum only, since hd omits `NumberError` too.

| # | Case (crate) | Rust | V1 | V2a | V2b |
| --- | --- | --- | --- | --- | --- |
| 1 | Regex error with private kind (grep-regex, ripgrep) | hand-written; 0; 21 | friction; 1; 16 | works; 0; 17 | works; 0; 17 |
| 2 | Layered errors with context (cargo) | hand-written; 0; 30 | breaks; 0; 23 | breaks; 0; 23 | friction; 0; 21 |
| 3 | Data error with line, column, category (serde_json) | hand-written; 0; 41 | works (by hand); 0; 41 | works (by hand); 0; 41 | works (by hand); 0; 41 |
| 4 | Kind plus optional source plus URL (reqwest) | hand-written; 0; 24 | friction; 1; 24 | works; 0; 25 | works; 0; 25 |
| 5 | Kind plus optional inner (std::io::Error) | hand-written; 0; 25 | breaks; 0; 27 | breaks; 0; 27 | works; 0; 23 |
| 6 | `RuleCoreError` (ast-grep) | thiserror; 13; 17 | works; 8; 17 | works; 1; 20 | works; 1; 20 |
| 7 | thiserror-heavy app error, 12 variants, plus `TaskError[E]` | thiserror; 25; 36 | friction; 16; 34 | friction; 4; 43 | works; 1; 40 |
| 8 | A span on every variant (naga WGSL front end) | no `Display`; 0; 8 | friction; 6; 14 | friction; 0; 17 | friction; 0; 17 |
| 9 | Data-type error (globset, ripgrep) | hand-written; 0; 37 | friction; 1; 25 | works; 0; 29 | works; 0; 29 |
| 10 | Erased `Error`, `.context`, `find`, `downcast` | anyhow | works | works | works |
| 11 | `?` across three layers | thiserror | friction | friction | works |
| 12 | `?` in test blocks (decision 14) | `Result<(), Box<dyn Error>>` | friction | friction | friction |
| 13 | Entry-point chain printing (decision 13) | anyhow `{:?}` | friction | friction | friction |
| 14 | Boundary conversion to `ErrorReport` | n/a | friction | friction | friction |

Cases 10-14 exercise operations, not declarations, so they have no marker
or line counts; they reuse types from cases 6 and 7.

In short: the three variants agree wherever a wrapper does not forward its
payload's message. They split on wrappers. V1 and V2a cannot express a
transparent wrapper that carries more than the payload (cargo's
`ManifestError`, `io::Error`'s custom errors), and they hide a transparent
payload from `find` and `root_cause`. V2b expresses both and keeps `find`
exact, but it moves duplicate-skipping into printers, where a hand-written
printer must repeat it. V1 needs the most markers and usually the fewest
lines; V2 needs a `Display` impl per type, which V1 also needs for any
kind enum that is not itself an error (cases 1, 4, 9). The problems that
matter most are shared by all three: automatic `From` and automatic cause
have no opt-out, and they change when an upstream type starts
implementing `Error`.

## Cases

### 1. A Regex Error With A Private Kind (grep-regex, ripgrep)

grep-regex's `Error` is a data type with one private `kind` field and a
public `#[non_exhaustive]` kind enum; `Display` matches on the kind.
ripgrep's `main` walks the chain for a broken pipe and exits quietly.

```rust
#[derive(Clone, Debug)]
pub struct Error { kind: ErrorKind }

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    Regex(String),
    NotAllowed(String),
    InvalidLineTerminator(u8),
    Banned(u8),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use bstr::ByteSlice;
        match self.kind {
            ErrorKind::Regex(ref s) => write!(f, "{}", s),
            ErrorKind::NotAllowed(ref lit) => write!(f, "the literal {:?} is not allowed in a regex", lit),
            ErrorKind::InvalidLineTerminator(byte) => write!(f, "line terminators must be ASCII, but {} is not", [byte].as_bstr()),
            ErrorKind::Banned(byte) => write!(f, "pattern contains {:?} but it is impossible to match", [byte].as_bstr()),
        }
    }
}
```

V1. The data type's message needs the kind's text. The kind cannot use
`@message` without `@derive(Error)`, and deriving `Error` on it would make
it the automatic cause (below), so its `Display` is hand-written anyway:

```text
use std.error.Error

@derive(Error)
@message("$kind")
pub data RegexError:
    kind: RegexErrorKind

pub enum RegexErrorKind:
    Regex(message: string)
    NotAllowed(literal: string)
    InvalidLineTerminator(byte: u8)
    Banned(byte: u8)

impl Display for RegexErrorKind:
    fn to_string(self) -> string:
        match self:
            RegexErrorKind.Regex(message) => message
            RegexErrorKind.NotAllowed(literal) => "the literal ${quoted(literal)} is not allowed in a regex"
            RegexErrorKind.InvalidLineTerminator(byte) => "line terminators must be ASCII, but ${escape_byte(byte)} is not"
            RegexErrorKind.Banned(byte) => "pattern contains ${quoted(escape_byte(byte))} but it is impossible to match"
```

The tempting V1 spelling derives the kind too, and it silently changes the
chain:

```text
@derive(Error)
pub enum RegexErrorKind:
    @message("the literal ${quoted(literal)} is not allowed in a regex")
    NotAllowed(literal: string)

@derive(Error)
@message("$kind")
pub data RegexError:
    kind: RegexErrorKind      # now implements Error, so it is the automatic cause
```

Printed: `the literal "a\nb" is not allowed in a regex`, then `caused by:`
and the same sentence. Nothing marks `kind` as "not a cause"
([problem 3](#3-automatic-cause-has-no-opt-out)).

V2a and V2b are identical here. The derive adds nothing (no cause, no
`From`), so the one-line `impl Error` is the whole error part:

```text
use std.error.Error

pub data RegexError:
    kind: RegexErrorKind

impl Display for RegexError:
    fn to_string(self) -> string: self.kind.to_string()

impl Error for RegexError

pub enum RegexErrorKind:
    Regex(message: string)
    NotAllowed(literal: string)
    InvalidLineTerminator(byte: u8)
    Banned(byte: u8)

impl Display for RegexErrorKind:
    fn to_string(self) -> string:
        match self:
            RegexErrorKind.Regex(message) => message
            RegexErrorKind.NotAllowed(literal) => "the literal ${quoted(literal)} is not allowed in a regex"
            RegexErrorKind.InvalidLineTerminator(byte) => "line terminators must be ASCII, but ${escape_byte(byte)} is not"
            RegexErrorKind.Banned(byte) => "pattern contains ${quoted(escape_byte(byte))} but it is impossible to match"
```

ripgrep's `main`, in every variant:

```text
use std.error.Error

fn is_broken_pipe(error: Error) -> bool:
    match error.find[IoError]():
        .Some(io) =>
            match io.kind():
                IoErrorKind.BrokenPipe => true
                _ => false
        .None => false

pub fn main!() -> Result[void, Error] $ Console + FsRead:
    match run!():
        .Ok(_) => .Ok()
        .Err(error) =>
            if is_broken_pipe(error):
                return .Ok()
            .Err(error)
```

- **Generated.** V1: `impl Display for RegexError` (`kind.to_string()`)
  and `impl Error` with `cause()` `.None`. V2: nothing.
- **Behavior.** `?` into `Result[T, Error]` by assignability; `find`
  reaches an `IoError` anywhere in the chain; `report_of` gives one line.
- **Breakage.** ripgrep exits with status 2 on an error; hd's entry point
  always exits with 1 ([problem 12](#12-the-entry-point-fixes-exit-status-and-format)).
  Rust's `{:?}` quoting has no hd counterpart, because interpolation never
  falls back to debug output (05 Literals); `quoted` and `escape_byte` are
  helpers in every variant. `#[non_exhaustive]` has no counterpart
  ([problem 16](#16-no-non-exhaustive-error-enums)).

### 2. Layered Errors With Context (cargo)

cargo returns `anyhow::Result` almost everywhere and adds context at each
layer. Its own error types are data types that forward `Display` and
`source()` to an inner error and exist to be found with `downcast_ref`:
`ManifestError` ("adds no displayable info of its own" and carries the
manifest path), `InternalError`, and `VerboseError`. Its printer walks the
chain and stops at a `VerboseError` unless `--verbose`.

```rust
pub struct ManifestError { cause: Error, manifest: PathBuf }
impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { self.cause.source() }
}
impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.cause.fmt(f) }
}

pub struct InternalError { inner: Error }
impl std::error::Error for InternalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { self.inner.source() }
}
impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.inner.fmt(f) }
}

pub struct HttpNotSuccessful {
    pub code: u32,
    pub url: String,
    pub ip: Option<String>,
    pub body: Vec<u8>,
    pub headers: Vec<String>,
}
impl std::error::Error for HttpNotSuccessful {}
impl fmt::Display for HttpNotSuccessful {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let url = &self.url;
        let ip = self.ip.as_deref().map(|ip| format!(" (IP {ip})")).unwrap_or_default();
        write!(f, "failed to get successful HTTP response from `{url}`{ip}, got {}", self.code)?;
        write!(f, "\nbody:\n{}", String::from_utf8_lossy(&self.body))
    }
}
```

V1 and V2a cannot express `ManifestError` or `InternalError` with the
derive: `@transparent` applies to a one-payload variant, and these are
data types, one of them with a second field. Deriving with a forwarding
message duplicates the line in the chain, because the `Error`-typed field
becomes the cause:

```text
@derive(Error)
@message("$cause")                          # V1: forwards the message
pub data ManifestError:
    cause: Error                            # automatic cause: printed twice
    manifest: string
```

So V1 and V2a hand-write both impls, exactly as cargo does:

```text
use std.error.Error

pub data ManifestError:
    cause: Error
    manifest: string

impl Display for ManifestError:
    fn to_string(self) -> string: self.cause.to_string()

impl Error for ManifestError:
    fn cause(self) -> Error?: self.cause.cause()

pub data InternalError:
    inner: Error

impl Display for InternalError:
    fn to_string(self) -> string: self.inner.to_string()

impl Error for InternalError:
    fn cause(self) -> Error?: self.inner.cause()

pub data HttpNotSuccessful:
    pub code: u32
    pub url: string
    pub ip: string?
    pub body: string
    pub headers: List[string]

impl Display for HttpNotSuccessful:
    fn to_string(self) -> string:
        "failed to get successful HTTP response from `${self.url}`${ip_note(self.ip)}, got ${self.code}\nbody:\n${self.body}"

impl Error for HttpNotSuccessful
```

V2b derives the two wrappers: the cause is the inner error, and the chain
printer skips the repeated line:

```text
use std.error.Error

@derive(Error)
pub data ManifestError:
    cause: Error
    manifest: string

impl Display for ManifestError:
    fn to_string(self) -> string: self.cause.to_string()

@derive(Error)
pub data InternalError:
    inner: Error

impl Display for InternalError:
    fn to_string(self) -> string: self.inner.to_string()
```

The layers, in every variant:

```text
use std.error.{Error, chain}

fn read_manifest(path: string) -> Result[Manifest, ManifestError]:
    text := read_file(path).map_err(fn(e: FsError) -> ManifestError: ManifestError { cause: e, manifest: path })?
    parse_toml(text).map_err(fn(e: TomlError) -> ManifestError: ManifestError { cause: e, manifest: path })

fn load_member(name: string, path: string) -> Result[Manifest, Error]:
    read_manifest(path).context("failed to load manifest for workspace member `$name`")

fn manifest_paths(error: Error) -> List[string]:
    let found: mut List[string] = []
    for part in chain(error):
        match part.downcast[ManifestError]():
            .Some(manifest) => found.append(manifest.manifest)
            .None => pass
    found

fn print_error(error: Error, verbose: bool) -> void:
    for part in chain(error):
        match part.downcast[VerboseError]():
            .Some(_) =>
                if verbose == false:
                    return
            .None => pass
        println(part.to_string())
```

- **Generated.** V1, V2a: nothing (hand-written). V2b: `impl Error for
  ManifestError` with `cause()` `.Some(self.cause)`, and the same for
  `InternalError`.
- **`?`.** `ManifestError` has two fields, so no `From` exists in any
  variant: every site maps with a closure, as cargo's `ManifestError::new`
  calls do. `.context` reaches `Result[T, Error]`.
- **`find`, `chain`.** `manifest_paths` works in all variants for the
  outermost `ManifestError`. A `ManifestError` whose `cause` is directly
  another `ManifestError` (nested workspace members) is skipped by a walk
  from the top (`chain(error)`, `find`) in V1 and V2a, because the outer
  one's `cause()` returns the inner one's cause. cargo's
  `manifest_causes` walks `chain(self.cause)`, which starts at the inner
  error, and that works in all variants. In V2b the walk from the top
  sees it too.
- **Printing.** cargo prints its own chain (`Caused by:` blocks,
  `VerboseError` cut-off). In V2b, `chain` still holds the repeated
  message, so `print_error` prints it twice unless it repeats the
  standard printer's rule ([problem 4](#4-v2bs-duplicate-skipping-lives-only-in-printers)).
- **Breakage.** cargo exits with 101 and formats its own `Caused by:`
  output; the entry point's fixed format and status cannot do either
  ([problem 12](#12-the-entry-point-fixes-exit-status-and-format)).

### 3. A Data Error With Line, Column, And Category (serde_json)

serde_json's `Error` boxes `{ code, line, column }`. `Display` appends the
position when `line` is not 0; `source()` looks inside the code for an I/O
error and returns *its* source (transparent); `classify()` maps codes to
`Category`. There is no `From<io::Error>`: `Error::io` sets line 0. The
real `ErrorCode` has about 25 variants; this subset has 8 in both
languages.

```rust
pub struct Error { err: Box<ErrorImpl> }
struct ErrorImpl { code: ErrorCode, line: usize, column: usize }
pub(crate) enum ErrorCode {
    Message(Box<str>),
    Io(io::Error),
    EofWhileParsingList,
    EofWhileParsingValue,
    ExpectedColon,
    ExpectedSomeValue,
    TrailingComma,
    RecursionLimitExceeded,
}
pub enum Category { Io, Syntax, Data, Eof }
impl Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorCode::Message(msg) => f.write_str(msg),
            ErrorCode::Io(err) => Display::fmt(err, f),
            ErrorCode::EofWhileParsingList => f.write_str("EOF while parsing a list"),
            ErrorCode::EofWhileParsingValue => f.write_str("EOF while parsing a value"),
            ErrorCode::ExpectedColon => f.write_str("expected `:`"),
            ErrorCode::ExpectedSomeValue => f.write_str("expected value"),
            ErrorCode::TrailingComma => f.write_str("trailing comma"),
            ErrorCode::RecursionLimitExceeded => f.write_str("recursion limit exceeded"),
        }
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.err.line == 0 { Display::fmt(&self.err.code, f) }
        else { write!(f, "{} at line {} column {}", self.err.code, self.err.line, self.err.column) }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.err.code { ErrorCode::Io(err) => err.source(), _ => None }
    }
}
impl From<Error> for io::Error {
    fn from(j: Error) -> Self { /* Io code unwrapped, else InvalidData */ }
}
```

The cause lives inside a member's variant, which no variant's inference
can see: the automatic cause looks at members whose *type* implements
`Error`, and `code` does not. Hand-writing `cause()` next to
`@derive(Error)` is `overlapping-impl` (a generated impl is an ordinary
impl), so all three variants write every impl by hand. The code is the
same in all three:

```text
use std.error.Error
use std.convert.From

pub data JsonError:
    code: JsonErrorCode
    line: i64
    column: i64

pub enum JsonErrorCode:
    Message(text: string)
    Io(error: IoError)
    EofWhileParsingList
    EofWhileParsingValue
    ExpectedColon
    ExpectedSomeValue
    TrailingComma
    RecursionLimitExceeded

pub enum Category:
    Io
    Syntax
    Data
    Eof

impl Display for JsonErrorCode:
    fn to_string(self) -> string:
        match self:
            JsonErrorCode.Message(text) => text
            JsonErrorCode.Io(error) => error.to_string()
            JsonErrorCode.EofWhileParsingList => "EOF while parsing a list"
            JsonErrorCode.EofWhileParsingValue => "EOF while parsing a value"
            JsonErrorCode.ExpectedColon => "expected `:`"
            JsonErrorCode.ExpectedSomeValue => "expected value"
            JsonErrorCode.TrailingComma => "trailing comma"
            JsonErrorCode.RecursionLimitExceeded => "recursion limit exceeded"

impl Display for JsonError:
    fn to_string(self) -> string:
        if self.line == 0:
            return self.code.to_string()
        "${self.code} at line ${self.line} column ${self.column}"

impl Error for JsonError:
    fn cause(self) -> Error?:
        match self.code:
            JsonErrorCode.Io(error) => error.cause()
            _ => .None

impl From[JsonError] for IoError:
    fn from(value: JsonError) -> IoError: IoError.Custom(IoErrorKind.InvalidData, value)
```

- **Generated.** Nothing, in any variant.
- **Behavior.** As in Rust. `impl From[JsonError] for IoError` is legal:
  the package that owns `JsonError` owns a trait argument (09 Conversion
  Trait). `classify()` is an ordinary exhaustive `match`, so a new code
  forces a category, as in Rust.
- **Breakage.** None beyond "the derive does not help". The wrapper
  idiom that [Derivation Stress Test 2, 1.5](DERIVATION_STRESS_TEST_2.md#15-common-enum-fields-a-span-on-every-variant)
  recommends for located errors is exactly serde_json's shape, and it is
  the shape the derive cannot serve, because the cause sits inside the
  kind ([problem 3](#3-automatic-cause-has-no-opt-out) asks whether a
  cause may be named through a member). V1's `@message` could produce
  the position text (`@message("${position_text(code, line, column)}")`),
  but only with the derive, which is ruled out here.

### 4. Kind Plus Optional Source Plus URL (reqwest)

reqwest's `Error` boxes `{ kind, source: Option<BoxError>, url:
Option<Url> }`. `Display` writes a phrase per kind and appends
` for url (...)`; `source()` returns the boxed source; `is_timeout()`
walks the source chain for hyper's `TimedOut` or an I/O `TimedOut`.

```rust
pub struct Error { inner: Box<Inner> }
struct Inner { kind: Kind, source: Option<BoxError>, url: Option<Url> }
pub(crate) enum Kind { Builder, Request, Redirect, Status(StatusCode), Body, Decode, Upgrade }
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.inner.kind {
            Kind::Builder => f.write_str("builder error")?,
            Kind::Request => f.write_str("error sending request")?,
            Kind::Redirect => f.write_str("error following redirect")?,
            Kind::Status(ref code) => {
                let prefix = if code.is_client_error() { "HTTP status client error" } else { "HTTP status server error" };
                write!(f, "{prefix} ({code})")?;
            }
            Kind::Body => f.write_str("request or response body error")?,
            Kind::Decode => f.write_str("error decoding response body")?,
            Kind::Upgrade => f.write_str("error upgrading connection")?,
        };
        if let Some(url) = &self.inner.url { write!(f, " for url ({})", url)?; }
        Ok(())
    }
}
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> { self.inner.source.as_ref().map(|e| &**e as _) }
}
```

V1 (the kind is `Display`-only, so its text is hand-written):

```text
use std.error.Error

@derive(Error)
@message("$kind${url_note(url)}")
pub data HttpError:
    kind: HttpErrorKind
    source: Error?
    url: Url?

pub enum HttpErrorKind:
    Builder
    Request
    Redirect
    Status(code: u16)
    Body
    Decode
    Upgrade

impl Display for HttpErrorKind:
    fn to_string(self) -> string:
        match self:
            HttpErrorKind.Builder => "builder error"
            HttpErrorKind.Request => "error sending request"
            HttpErrorKind.Redirect => "error following redirect"
            HttpErrorKind.Status(code) => "${status_class(code)} ($code)"
            HttpErrorKind.Body => "request or response body error"
            HttpErrorKind.Decode => "error decoding response body"
            HttpErrorKind.Upgrade => "error upgrading connection"
```

V2a and V2b replace the `@message` line with:

```text
impl Display for HttpError:
    fn to_string(self) -> string: "${self.kind}${url_note(self.url)}"
```

and keep `@derive(Error)` for the cause. The methods, in every variant:

```text
use std.error.{Error, chain}

impl HttpError:
    pub fn is_timeout(self) -> bool:
        for part in chain(self):
            if part.downcast[TimedOut]().is_some():
                return true
            match part.downcast[IoError]():
                .Some(io) =>
                    match io.kind():
                        IoErrorKind.TimedOut => return true
                        _ => pass
                .None => pass
        false

    pub fn without_url(self) -> HttpError:
        HttpError { ...self, url: .None }
```

- **Generated.** `impl Error` with `cause()` returning `self.source`
  (`source: Error?` is `E?` with `E` the erased `Error`, which satisfies
  `E < Error` by decision 4). V1 also generates `Display`.
- **Behavior.** As in Rust. `chain(self)` starts at `self` where Rust
  starts at `self.source()`; the extra part is harmless here.
- **Breakage.** `HttpError` holds an erased `Error`, so it is not
  boundary-safe: a registered tool cannot return `Result[T, HttpError]`,
  and std's `HttpError` domain enum could not adopt reqwest's shape
  ([problem 13](#13-an-erased-member-makes-an-error-type-non-boundary-safe)).
  In V1 the message scope includes a field named `source`, which is also
  the name of a marker; they do not collide, since markers are decorators.

### 5. Kind Plus Optional Inner (std::io::Error)

`io::Error` is an OS code, a bare kind, a static message, or a custom
error with a kind. `Display` for a custom error is the inner error's
message, and `source()` is the inner error's *source*: `io::Error` is
transparent over its custom payload, which also carries a kind.

```rust
pub struct Error { repr: Repr }
enum ErrorData<C> { Os(i32), Simple(ErrorKind), SimpleMessage(&'static SimpleMessage), Custom(C) }
struct Custom { kind: ErrorKind, error: Box<dyn error::Error + Send + Sync> }
pub enum ErrorKind { NotFound, PermissionDenied, TimedOut, BrokenPipe, UnexpectedEof, InvalidData, Other }
impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr.data() {
            ErrorData::Os(code) => { let detail = sys::os::error_string(code); write!(fmt, "{detail} (os error {code})") }
            ErrorData::Custom(ref c) => c.error.fmt(fmt),
            ErrorData::Simple(kind) => write!(fmt, "{}", kind.as_str()),
            ErrorData::SimpleMessage(msg) => msg.message.fmt(fmt),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self.repr.data() {
            ErrorData::Os(..) | ErrorData::Simple(..) | ErrorData::SimpleMessage(..) => None,
            ErrorData::Custom(c) => c.error.source(),
        }
    }
}
impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error { Error { repr: Repr::new_simple(kind) } }
}
```

V1 and V2a: `Custom(kind, error)` has two members, so it cannot be
`@transparent`. Deriving with a forwarding message prints the inner line
twice (the `error` member is the automatic cause), so the error part is
hand-written; the kind still gets a per-value common field:

```text
use std.error.Error
use std.convert.From

pub enum IoError(kind: IoErrorKind):
    Os(code: i32) -> IoError(decode_kind(code))
    Simple(k: IoErrorKind) -> IoError(k)
    Message(k: IoErrorKind, message: string) -> IoError(k)
    Custom(k: IoErrorKind, error: Error) -> IoError(k)

impl Display for IoError:
    fn to_string(self) -> string:
        match self:
            IoError.Os(code) => "${os_error_string(code)} (os error $code)"
            IoError.Simple(k) => k.to_string()
            IoError.Message(_, message) => message
            IoError.Custom(_, error) => error.to_string()

impl Error for IoError:
    fn cause(self) -> Error?:
        match self:
            IoError.Custom(_, error) => error.cause()
            _ => .None

impl From[IoErrorKind] for IoError:
    fn from(value: IoErrorKind) -> IoError: IoError.Simple(value)

pub enum IoErrorKind:
    NotFound
    PermissionDenied
    TimedOut
    BrokenPipe
    UnexpectedEof
    InvalidData
    Other
```

V2b derives the cause (`error` is the only `Error`-typed member), and the
printer skips the repeated line:

```text
@derive(Error)
pub enum IoError(kind: IoErrorKind):
    Os(code: i32) -> IoError(decode_kind(code))
    Simple(k: IoErrorKind) -> IoError(k)
    Message(k: IoErrorKind, message: string) -> IoError(k)
    Custom(k: IoErrorKind, error: Error) -> IoError(k)
```

with the same `Display`, `From[IoErrorKind]`, and kind enum.

- **Generated.** V1, V2a: nothing. V2b: `cause()` with
  `IoError.Custom(_, error) => .Some(error)`, other arms `.None`. No
  `From`: `IoErrorKind` does not implement `Error`, and the common field
  `kind` has no default, which blocks promotion anyway (R12).
- **`find`.** `error.find[MyCodecError]()` on an `IoError.Custom` finds
  the payload in V2b only. In V1 and V2a it finds the payload's cause, not
  the payload; Rust users write `get_ref()` then `downcast_ref` for this,
  which in hd is `io.get_ref()` then `downcast`.
- **`root_cause`.** For a custom payload with no cause, V1 and V2a return
  the `IoError` itself (its `cause()` is the payload's `.None`); V2b
  returns the payload.
- **Breakage.** In all variants `IoError` holds an erased `Error`, so it
  is not boundary-safe ([problem 13](#13-an-erased-member-makes-an-error-type-non-boundary-safe));
  hd's std keeps boundary-safe domain enums (`FsError`) instead. The
  common field needs each variant to copy a payload through `->` (named
  `k`, because a payload may not reuse a shared field's name, 08), which
  assumes payload names are in scope there
  ([problem 9](#9-common-fields-per-value-data-and-decision-10s-two-outcomes)).

### 6. `RuleCoreError` (ast-grep)

The Current Design's reference case, from ast-grep-config's
`rule_core.rs`.

```rust
#[derive(Debug, Error)]
pub enum RuleCoreError {
    #[error("Fail to parse yaml as RuleConfig")]
    Yaml(#[from] YamlError),
    #[error("`utils` is not configured correctly.")]
    Utils(#[source] RuleSerializeError),
    #[error("`rule` is not configured correctly.")]
    Rule(#[from] RuleSerializeError),
    #[error("`constraints` is not configured correctly.")]
    Constraints(#[source] RuleSerializeError),
    #[error("`transform` is not configured correctly.")]
    Transform(#[from] TransformError),
    #[error("`fix` pattern is invalid.")]
    Fixer(#[from] FixerError),
    #[error("Undefined meta var `{0}` used in `{1}`.")]
    UndefinedMetaVar(String, &'static str),
}
```

V1:

```text
use std.error.Error

@derive(Error)
pub enum RuleCoreError:
    @message("Fail to parse yaml as RuleConfig")
    Yaml(error: YamlError)
    @message("`utils` is not configured correctly.")
    Utils(error: RuleSerializeError)
    @message("`rule` is not configured correctly.")
    @from
    Rule(error: RuleSerializeError)
    @message("`constraints` is not configured correctly.")
    Constraints(error: RuleSerializeError)
    @message("`transform` is not configured correctly.")
    Transform(error: TransformError)
    @message("`fix` pattern is invalid.")
    Fixer(error: FixerError)
    @message("Undefined meta var `$var` used in `$context`.")
    UndefinedMetaVar(var: string, context: string)
```

V2a and V2b (identical: no wrapper here):

```text
use std.error.Error

@derive(Error)
pub enum RuleCoreError:
    Yaml(error: YamlError)
    Utils(error: RuleSerializeError)
    @from
    Rule(error: RuleSerializeError)
    Constraints(error: RuleSerializeError)
    Transform(error: TransformError)
    Fixer(error: FixerError)
    UndefinedMetaVar(string, string)

impl Display for RuleCoreError:
    fn to_string(self) -> string:
        match self:
            RuleCoreError.Yaml(_) => "Fail to parse yaml as RuleConfig"
            RuleCoreError.Utils(_) => "`utils` is not configured correctly."
            RuleCoreError.Rule(_) => "`rule` is not configured correctly."
            RuleCoreError.Constraints(_) => "`constraints` is not configured correctly."
            RuleCoreError.Transform(_) => "`transform` is not configured correctly."
            RuleCoreError.Fixer(_) => "`fix` pattern is invalid."
            RuleCoreError.UndefinedMetaVar(var, context) => "Undefined meta var `$var` used in `$context`."
```

- **Generated (all).** `impl Error` whose `cause()` returns
  `.Some(error)` for the six wrapping variants and `.None` for
  `UndefinedMetaVar`; `From[YamlError]`, `From[RuleSerializeError]` (to
  `Rule`), `From[TransformError]`, `From[FixerError]`. V1 also generates
  `Display`.
- **`?`, `find`, `chain`, `root_cause`, `report_of`.** As in Rust.
  `find[RuleSerializeError]` finds the payload of any of the three
  sharing variants.
- **Unnamed payloads.** ast-grep's `UndefinedMetaVar(String, &'static
  str)` is positional. V1 must name the payloads so `@message` can
  interpolate them (R18); V2 keeps them positional, because the `match`
  binds any name.
- **Breakage (all).** `?` on a `RuleSerializeError` always yields `Rule`,
  even where `Utils` was meant (R11, still open). If the enum first had
  only `Rule(error: RuleSerializeError)`, it got `From` with no marker;
  adding `Utils` later removes that `From` with a lint note, and every
  `?` that relied on it becomes `invalid-result-propagation`
  ([problem 5](#5-adding-a-variant-can-remove-a-from)).

### 7. A thiserror-Heavy Application Error

A composite of common thiserror application shapes (the thiserror README's
`DataStoreError`, plus the patterns CLI tools use): plain wrappers,
context-plus-cause variants, a recursive retry variant, two transparent
wrappers, an `anyhow` catch-all, a variant with two errors, and a generic
task error.

```rust
#[derive(Debug, Error)]
pub enum AppError {
    #[error("i/o error on {path}")]
    Io { path: PathBuf, #[source] source: io::Error },
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("failed to parse {path}")]
    Parse { path: PathBuf, #[source] source: toml::de::Error },
    #[error("request to {url} failed")]
    Http { url: String, #[source] source: reqwest::Error },
    #[error("database error")]
    Db(#[from] sqlx::Error),
    #[error("migration {version} failed")]
    Migration { version: i64, #[source] source: sqlx::Error, rollback: Option<sqlx::Error> },
    #[error("task {id} failed after {attempts} attempts")]
    Retry { id: u64, attempts: u32, #[source] last: Box<AppError> },
    #[error("invalid argument: {0}")]
    InvalidArg(String),
    #[error("{kind} `{name}` not found")]
    NotFound { kind: &'static str, name: String },
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum TaskError<E: std::error::Error + 'static> {
    #[error("task cancelled")]
    Cancelled,
    #[error("task failed")]
    Failed(#[source] E),
    #[error("task panicked: {0}")]
    Panicked(String),
}
```

V1:

```text
use std.error.Error

@derive(Error)
pub enum AppError:
    @message("i/o error on $path")
    Io(path: string, error: IoError)
    @transparent
    Config(error: ConfigError)
    @message("failed to parse $path")
    Parse(path: string, error: TomlError)
    @message("request to $url failed")
    Http(url: string, error: HttpError)
    @message("database error")
    Db(error: DbError)
    @message("migration $version failed")
    Migration(version: i64, @source error: DbError, rollback: DbError?)   # hypothetical syntax
    @message("task $id failed after $attempts attempts")
    Retry(id: u64, attempts: u32, last: AppError)
    @message("invalid argument: $reason")
    InvalidArg(reason: string)
    @message("$kind `$name` not found")
    NotFound(kind: string, name: string)
    @message("timed out after ${seconds}s")
    Timeout(seconds: i64)
    @transparent
    Json(error: JsonError)
    @transparent
    Other(error: Error)

@derive(Error)
pub enum TaskError[E]:
    @message("task cancelled")
    Cancelled
    @message("task failed")
    Failed(error: E)
    @message("task panicked: $message")
    Panicked(message: string)
```

V2 (V2b drops the three `# V2a only` lines):

```text
use std.error.Error

@derive(Error)
pub enum AppError:
    Io(path: string, error: IoError)
    @transparent                                                        # V2a only
    Config(error: ConfigError)
    Parse(path: string, error: TomlError)
    Http(url: string, error: HttpError)
    Db(error: DbError)
    Migration(version: i64, @source error: DbError, rollback: DbError?)   # hypothetical syntax
    Retry(id: u64, attempts: u32, last: AppError)
    InvalidArg(string)
    NotFound(kind: string, name: string)
    Timeout(seconds: i64)
    @transparent                                                        # V2a only
    Json(error: JsonError)
    @transparent                                                        # V2a only
    Other(error: Error)

impl Display for AppError:
    fn to_string(self) -> string:
        match self:
            AppError.Io(path, _) => "i/o error on $path"
            AppError.Config(error) => error.to_string()
            AppError.Parse(path, _) => "failed to parse $path"
            AppError.Http(url, _) => "request to $url failed"
            AppError.Db(_) => "database error"
            AppError.Migration(version, _, _) => "migration $version failed"
            AppError.Retry(id, attempts, _) => "task $id failed after $attempts attempts"
            AppError.InvalidArg(reason) => "invalid argument: $reason"
            AppError.NotFound(kind, name) => "$kind `$name` not found"
            AppError.Timeout(seconds) => "timed out after ${seconds}s"
            AppError.Json(error) => error.to_string()
            AppError.Other(error) => error.to_string()

@derive(Error)
pub enum TaskError[E]:
    Cancelled
    Failed(error: E)
    Panicked(string)

impl[E] Display for TaskError[E]:
    fn to_string(self) -> string:
        match self:
            TaskError.Cancelled => "task cancelled"
            TaskError.Failed(_) => "task failed"
            TaskError.Panicked(message) => "task panicked: $message"
```

What the derive generates for `AppError`, in V2b (V1 and V2a differ only
in the three transparent arms, which return `error.cause()`):

```text
impl Error for AppError:
    fn cause(self) -> Error?:
        match self:
            AppError.Io(_, error) => .Some(error)
            AppError.Config(error) => .Some(error)
            AppError.Parse(_, error) => .Some(error)
            AppError.Http(_, error) => .Some(error)
            AppError.Db(error) => .Some(error)
            AppError.Migration(_, error, _) => .Some(error)
            AppError.Retry(_, _, last) => .Some(last)
            AppError.InvalidArg(_) => .None
            AppError.NotFound(_, _) => .None
            AppError.Timeout(_) => .None
            AppError.Json(error) => .Some(error)
            AppError.Other(error) => .Some(error)

impl From[ConfigError] for AppError:
    fn from(value: ConfigError) -> AppError: AppError.Config(value)

impl From[DbError] for AppError:
    fn from(value: DbError) -> AppError: AppError.Db(value)

impl From[JsonError] for AppError:
    fn from(value: JsonError) -> AppError: AppError.Json(value)

impl From[Error] for AppError:
    fn from(value: Error) -> AppError: AppError.Other(value)

impl[E < Error] Error for TaskError[E]:
    fn cause(self) -> Error?:
        match self:
            TaskError.Failed(error) => .Some(error)
            _ => .None
```

- **`?`.** `DbError` converts to `Db` although `Migration` also holds a
  `DbError`: `Migration` has three payloads, so it is not a candidate and
  there is no conflict. `IoError` has no `From` (its variant has a path),
  as in Rust. `Other` catches only an already erased `Error`: a `?` on a
  `YamlError` is still `invalid-result-propagation` (one step, decision
  5), as it is in Rust with `#[from] anyhow::Error`.
- **Recursion.** `Retry(..., last: AppError)` is fine: the cause is the
  earlier attempt, and `chain` walks it. In V1, `@message` may interpolate
  `$last` (not `$self`). A one-payload `Nested(error: AppError)` would get
  `impl From[AppError] for AppError`: legal (no reflexive impl exists),
  never used by `?` (step 1 wins), and a surprise for a direct
  `AppError::from(e)`.
- **`find`.** In V1 and V2a, `error.find[ConfigError]()` on
  `AppError.Config(inner)` returns `.None` (R10); so do
  `find[JsonError]()` and, for `Other`, a `downcast` of the erased
  payload. V2b returns the payload.
- **V2a drift.** `@transparent` changes only `cause()`. If the `Config`
  arm says `"invalid configuration"` instead of `error.to_string()`, the
  inner message is printed nowhere: not by `Display`, and not by the chain,
  which skips it. Nothing checks the pairing
  ([problem 7](#7-v2as-transparent-is-not-checked-against-the-display-arm)).
- **Generic payload.** `TaskError[E].Failed` is never promoted (R6), so a
  function returning `Result[T, TaskError[FsError]]` maps by hand. 08
  requires an expected monomorphic function type for a generic
  constructor value, and `map_err[F, R]`'s parameter `fn(E) -> F` leaves
  `F` open, so `result.map_err(TaskError.Failed)` likely fails to infer;
  the closure `fn(e: FsError) -> TaskError[FsError]: TaskError.Failed(e)`
  works ([problem 15](#15-generic-constructors-as-mapping-functions)).
  In V1 the generated `Display` needs no bound (no message interpolates
  `E`); in V2 the author writes the header, here with no bound, and the
  generated `impl[E < Error] Error` finds it. Either works.
- **Evolution (all).** Adding `Plugin(error: Error)` makes a second
  one-payload variant whose payload is `Error`, which removes
  `From[Error]` with a lint note ([problem 5](#5-adding-a-variant-can-remove-a-from)).
  Hand-writing `impl From[DbError] for AppError` to map unique-key
  violations to a `Conflict` variant is `overlapping-impl` against the
  generated one, and no marker turns the generated one off
  ([problem 1](#1-automatic-from-has-no-opt-out)).
- **V1 defaults.** A variant added without `@message` compiles and
  displays as `Timeout` or `Db` ([problem 6](#6-v1-displays-a-variant-without-message-as-its-name)).
  In V2 the `Display` match stops compiling until the arm is written.

### 8. A Span On Every Variant (naga WGSL front end)

naga's WGSL parser error is an enum of about 80 variants, nearly all with
a `Span`; it has no `Display` and renders through `as_parse_error(source)`
because the message quotes the source text. `NumberError` is a thiserror
enum.

```rust
pub(crate) enum Error<'a> {
    Unexpected(Span, ExpectedToken<'a>),
    UnexpectedComponents(Span),
    BadNumber(Span, NumberError),
    UnknownIdent(Span, &'a str),
    UnknownType(Span),
    Redefinition { previous: Span, current: Span },
}

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum NumberError {
    #[error("invalid numeric literal format")]
    Invalid,
    #[error("numeric literal not representable by target type")]
    NotRepresentable,
}
```

The shared enum field is the natural hd home for "every variant has a
span", but shared data comes only from each variant's `->` clause, so a
per-value span is a payload that the clause copies. V1:

```text
use std.error.Error

@derive(Error)
pub enum WgslError(span: Span):
    @message("expected $expected")
    Unexpected(at: Span, expected: string) -> WgslError(at)
    @message("unexpected components")
    UnexpectedComponents(at: Span) -> WgslError(at)
    @message("invalid numeric literal")
    BadNumber(at: Span, error: NumberError) -> WgslError(at)
    @message("no definition in scope for identifier `$name`")
    UnknownIdent(at: Span, name: string) -> WgslError(at)
    @message("unknown type")
    UnknownType(at: Span) -> WgslError(at)
    @message("redefinition of a name")
    Redefinition(previous: Span, current: Span) -> WgslError(current)
```

V2 (identical in V2a and V2b):

```text
use std.error.Error

@derive(Error)
pub enum WgslError(span: Span):
    Unexpected(at: Span, expected: string) -> WgslError(at)
    UnexpectedComponents(at: Span) -> WgslError(at)
    BadNumber(at: Span, error: NumberError) -> WgslError(at)
    UnknownIdent(at: Span, name: string) -> WgslError(at)
    UnknownType(at: Span) -> WgslError(at)
    Redefinition(previous: Span, current: Span) -> WgslError(current)

impl Display for WgslError:
    fn to_string(self) -> string:
        match self:
            WgslError.Unexpected(_, expected) => "expected $expected"
            WgslError.UnexpectedComponents(_) => "unexpected components"
            WgslError.BadNumber(_, _) => "invalid numeric literal"
            WgslError.UnknownIdent(_, name) => "no definition in scope for identifier `$name`"
            WgslError.UnknownType(_) => "unknown type"
            WgslError.Redefinition(_, _) => "redefinition of a name"
```

Rendering with source text is an ordinary method in every variant, since
`Display` takes no arguments:

```text
impl WgslError:
    pub fn render(self, source: string) -> string:
        "${self.span.line(source)}:${self.span.column(source)}: $self"
```

- **Generated.** `impl Error` with `cause()` `.Some(error)` for
  `BadNumber`, `.None` elsewhere. No `From`: the common field `span` has
  no default, so no variant is promoted (R12), and `BadNumber` has two
  payloads anyway. naga builds these by hand too.
- **The shared field buys little.** Every variant repeats `at: Span` and
  `-> WgslError(at)`. What it buys is `error.span` without a `match`.
  It assumes the `->` arguments see the payload names, which 08 does not
  state ([problem 9](#9-common-fields-per-value-data-and-decision-10s-two-outcomes)).
- **The wrapper alternative** (`data WgslError: span, kind`, as serde_json
  does) loses the cause: `NumberError` sits inside the kind, which is
  case 3's shape, so `cause()` is hand-written and the derive is unused.
- **Record inconsistency.** Decision 10 says a promoted variant's common
  fields "must have defaults, otherwise the error names the missing one",
  and later that such a variant "gets no automatic `From` (a lint note
  says why)". Under the first, `WgslError` is an error; under the second,
  a lint note. This report follows the second.
- **Messages.** V1's `@message` sees `span` (common) and `at` (payload),
  the same value under two names. `render` interpolates `$self`, which is
  fine in a method other than `to_string`; V1 forbids `$self` only inside
  a message (R7).

### 9. A Data-Type Error (globset, ripgrep)

globset's `Error` is `{ glob: Option<String>, kind: ErrorKind }`;
`Display` prefixes `error parsing glob '...'` when the glob is known.

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error { glob: Option<String>, kind: ErrorKind }

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    InvalidRecursive,
    UnclosedClass,
    InvalidRange(char, char),
    UnopenedAlternates,
    UnclosedAlternates,
    NestedAlternates,
    DanglingEscape,
    Regex(String),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.glob {
            None => self.kind.fmt(f),
            Some(ref glob) => write!(f, "error parsing glob '{}': {}", glob, self.kind),
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ErrorKind::InvalidRecursive => write!(f, "invalid use of **; must be one path component"),
            ErrorKind::UnclosedClass => write!(f, "unclosed character class; missing ']'"),
            ErrorKind::InvalidRange(s, e) => write!(f, "invalid range; '{}' > '{}'", s, e),
            ErrorKind::UnopenedAlternates => write!(f, "unopened alternate group; missing '{{'"),
            ErrorKind::UnclosedAlternates => write!(f, "unclosed alternate group; missing '}}'"),
            ErrorKind::NestedAlternates => write!(f, "nested alternate groups are not allowed"),
            ErrorKind::DanglingEscape => write!(f, "dangling '\\'"),
            ErrorKind::Regex(ref err) => write!(f, "{}", err),
        }
    }
}
```

V1 (the optional prefix needs a helper, because a message is one string):

```text
use std.error.Error

@derive(Error)
@message("${glob_prefix(glob)}$kind")
pub data GlobError:
    glob: string?
    kind: GlobErrorKind

pub enum GlobErrorKind:
    InvalidRecursive
    UnclosedClass
    InvalidRange(start: char, end: char)
    UnopenedAlternates
    UnclosedAlternates
    NestedAlternates
    DanglingEscape
    Regex(message: string)

impl Display for GlobErrorKind:
    fn to_string(self) -> string:
        match self:
            GlobErrorKind.InvalidRecursive => "invalid use of **; must be one path component"
            GlobErrorKind.UnclosedClass => "unclosed character class; missing ']'"
            GlobErrorKind.InvalidRange(start, end) => "invalid range; '$start' > '$end'"
            GlobErrorKind.UnopenedAlternates => "unopened alternate group; missing '{'"
            GlobErrorKind.UnclosedAlternates => "unclosed alternate group; missing '}'"
            GlobErrorKind.NestedAlternates => "nested alternate groups are not allowed"
            GlobErrorKind.DanglingEscape => "dangling '\\'"
            GlobErrorKind.Regex(message) => message
```

V2a and V2b (no derive needed: no cause, no `From`):

```text
use std.error.Error

pub data GlobError:
    glob: string?
    kind: GlobErrorKind

impl Display for GlobError:
    fn to_string(self) -> string:
        match self.glob:
            .Some(glob) => "error parsing glob '$glob': ${self.kind}"
            .None => self.kind.to_string()

impl Error for GlobError
```

with the same kind enum and its `Display`.

- **Generated.** V1: `Display` and a `.None` cause. V2: nothing.
- **Behavior.** As in Rust, in all variants.
- **Breakage.** V1 again needs a hand-written `Display` for the kind
  (case 1), and its one-string message pushes a two-branch format into a
  helper. hd strings need no brace escaping (`'{'` is ordinary content),
  which is a small win over Rust's `'{{'` in all variants.

### 10. The Erased `Error`, `.context`, `find`, And `downcast`

An application that returns `Result[T, Error]` and adds context, using
the types of cases 6 and 7. `.context(...)` is written as a method, which
[STDLIB](STDLIB.md#stderror) plans once method syntax on `Result` is
settled; the free function `context(result, message)` types the same way.

```text
use std.error.{Error, chain, root_cause}

fn load_rules(path: string) -> Result[Rules, Error]:
    text := read_file(path).context("reading rules from $path")?
    rules := parse_rules(text).context("parsing rules in $path")?
    .Ok(rules)

fn exit_code(error: Error) -> i32:
    match error.find[FsError]():
        .Some(FsError.NotFound(_)) => 2
        .Some(_) => 3
        .None =>
            match root_cause(error).downcast[RuleSerializeError]():
                .Some(_) => 4
                .None => 1
```

- **`?`.** `.context` returns `Result[T, Error]`, so `?` is identity.
  `?` on `read_file(path)` without `.context` reaches `Error` by
  assignability.
- **`find`, `downcast`, `root_cause`.** Identical in the three variants
  when no transparent wrapper is on the path. Behind one (an
  `AppError.Other` holding the erased error, or an `AppError.Config`),
  V1 and V2a skip the payload: `find[ConfigError]` is `.None`, and when
  the payload has no cause, `root_cause` returns the wrapper, so the
  `downcast` above returns `.None`. V2b finds both. This is R10, and it
  also affects `root_cause`, which R10 did not list.
- **Cost (all).** `.context("reading rules from $path")` builds its string
  on the success path too; anyhow users write `.with_context(|| ...)` to
  avoid that ([problem 10](#10-context-builds-its-message-on-success)).
- **Bound (all).** `find[T < AnyRef + Inspectable]` works for enums today.
  [Enum Semantics](ENUM_SEMANTICS.md) recommends making enums values;
  under that change `find[FsError]` no longer satisfies `AnyRef`
  ([problem 14](#14-find-requires-anyref)).

### 11. `?` Across Three Layers

A library error, a loader error with a transparent wrapper, and an
application error, each converted by `?` once:

```text
use std.error.Error

@derive(Error)
pub enum LoadError:
    @message("cannot read $path")
    Read(path: string, error: FsError)
    @transparent
    Rules(error: RuleCoreError)

@derive(Error)
pub enum StartError:
    @message("failed to load rules")
    Load(error: LoadError)
    @message("cannot open cache")
    Cache(error: CacheError)

fn load(path: string) -> Result[Rules, LoadError]:
    text := read_file(path).map_err(fn(e: FsError) -> LoadError: LoadError.Read(path, e))?
    rules := parse_rules(text)?                  # From[RuleCoreError]: Rules
    .Ok(rules)

fn start(path: string) -> Result[App, StartError]:
    rules := load(path)?                         # From[LoadError]: Load
    .Ok(App { rules: rules })

pub fn main() -> Result[void, Error]:
    app := start("sgconfig.yml")?                # assignability: StartError to Error
    app.run()
    .Ok()
```

The same declarations in V2, with `Display` by hand (V2b drops the marked
line):

```text
use std.error.Error

@derive(Error)
pub enum LoadError:
    Read(path: string, error: FsError)
    @transparent                                 # V2a only
    Rules(error: RuleCoreError)

impl Display for LoadError:
    fn to_string(self) -> string:
        match self:
            LoadError.Read(path, _) => "cannot read $path"
            LoadError.Rules(error) => error.to_string()
```

For `RuleCoreError.Rule(RuleSerializeError.MissingPositiveMatcher)`:

| | V1 | V2a | V2b |
| --- | --- | --- | --- |
| `chain(error)` parts | `StartError.Load`, `LoadError.Rules`, `RuleSerializeError` | same as V1 | `StartError.Load`, `LoadError.Rules`, `RuleCoreError.Rule`, `RuleSerializeError` |
| Printed by `main` | `failed to load rules` / `` caused by: `rule` is not configured correctly. `` / `caused by: Rule must have one positive matcher.` | same | same (the repeated line is skipped) |
| `find[LoadError]` | found | found | found |
| `find[RuleCoreError]` | `.None` | `.None` | found |
| `root_cause` | `RuleSerializeError` | same | same |

- **`?`.** Each step is one `From` call or one assignability rule, as
  decision 5 requires. `start` can call `read_file` directly with `?`
  only if `StartError` has exactly one one-payload `FsError` variant; with
  two (say `Config(error: FsError)` and `Cache(error: FsError)`), neither
  converts
  ([problem 5](#5-adding-a-variant-can-remove-a-from)).
- **Breakage.** V1 and V2a: a caller of `start` that tests
  `find[RuleCoreError]` (to show rule-authoring help) never fires. The
  record accepts this (R10: follow thiserror); in Rust, `anyhow`'s
  `downcast_ref` has the same blind spot.

### 12. `?` In Test Blocks (Decision 14)

```text
use std.error.Error

test "rules load from the fixture":
    rules := load("fixtures/rules.yml")?
    assert(rules.length() == 2, "the fixture declares two rules")

test "a bad rule reports a rule error":
    match start("fixtures/bad.yml"):
        .Ok(_) => assert(false, "the bad fixture must fail")
        .Err(error) =>
            let erased: Error = error
            assert(erased.find[RuleCoreError]().is_some(), "the chain holds the rule error")

test "a parser that returns string errors":
    value := parse_number("12")?
    assert(value == 12, "parses twelve")
```

- **First test (all).** `LoadError` reaches the block's `Error` by
  assignability; a failure prints the chain as `main` does.
- **Second test.** Passes in V2b, fails in V1 and V2a: the transparent
  `LoadError.Rules` hides `RuleCoreError` from `find`. The test author has
  to know which variants are transparent to write it (downcast
  `StartError`, match `Load`, downcast `LoadError`, match `Rules`).
- **Third test (all).** If `parse_number` returns `Result[i64, string]`,
  the `?` is `invalid-result-propagation`: `string` does not implement
  `Error`, and no `From` into the erased `Error` can exist (a trait value
  is never an impl target). Rust's `Box<dyn Error>` accepts `String`
  through `From<String>`. Tests of string-error helpers need `.map_err`
  into some error type, or no `?` ([problem 11](#11-tests-cannot-propagate-non-error-errors)).

### 13. Entry-Point Chain Printing (Decision 13)

For case 11's failure, `main` prints (message, then each cause as
`caused by: ...`) the same three lines in all variants. They differ in
two situations:

```text
fn load_config(path: string) -> Result[Config, Error]:
    parse_config(path).context("loading config")

fn boot() -> Result[App, Error]:
    config := load_config("app.toml").context("loading config")?
    .Ok(App { config: config })
```

- **Repeated context.** Caller and callee add the same sentence, which is
  common in anyhow code. V1 and V2a print `loading config` twice; V2b
  prints it once, merging two real levels. `chain` still has both.
- **A wrapper with extra fields** (cargo's `ManifestError`, `io::Error`'s
  custom payload) prints its line twice in V1 and V2a if derived, which is
  why cases 2 and 5 hand-write it; V2b prints it once.
- **All variants.** The status is always 1 and the format is fixed, so
  ripgrep (status 2, quiet on a broken pipe) and cargo (status 101,
  `Caused by:` blocks) print and exit by hand
  ([problem 12](#12-the-entry-point-fixes-exit-status-and-format)).
  A chain can also be cyclic: an error type with a `mut` cause field can
  be made to point at itself, and the printer then never stops
  ([problem 17](#17-cyclic-chains)).

### 14. Boundary Conversion To `ErrorReport`

```text
use std.error.{Error, ErrorReport, report_of}

@tool
fn load_tool(path: string) -> Result[string, ErrorReport]:
    match start(path):
        .Ok(app) => .Ok(app.summary())
        .Err(error) => .Err(report_of(error))
```

(`@tool` is the Durable Replay sketch's decorator.)

- **`report_of` (all).** `ErrorReport { message, causes }` is
  boundary-safe. The STDLIB sketch fills `causes` from `chain(error)`,
  which starts with the error itself, so `causes[0]` repeats `message`
  ([problem 8](#8-errorreport-repeats-the-top-message)). V2b's skip rule has
  to be applied here too, and the sketch does not yet do it.
- **Typed errors at the boundary (all).** `StartError` is boundary-safe
  only if every payload is. `AppError` (case 7, `Other(error: Error)`),
  `HttpError` (case 4), and `IoError` (case 5) are not, so they cross
  only as reports ([problem 13](#13-an-erased-member-makes-an-error-type-non-boundary-safe)).
- **What is lost.** The report keeps strings only; a remote caller cannot
  `find` anything. That is decision 6's intent.

## Marker Scope

Only V1's `@message` reads names bound elsewhere: the payload members of
the variant on the following line, a data type's fields, and the enum's
common fields. The record grants this to that one intrinsic marker (E1),
and this report treats it as the record states it. The remaining markers,
in V1 and V2 alike:

- **`@from` and `@transparent`** take no argument. They attach to the
  variant they precede and name nothing, so they raise no scope question.
- **`@source` on a payload parameter** (decision 12,
  `Migration(version: i64, @source error: DbError, ...)`) attaches to the
  parameter it precedes and names nothing. The spelling the record's
  Current Design and R4 once used, `@source(error)` on the variant, would
  name a payload member before it is bound, the same kind of reference as
  `@message`. Decision 12's placement avoids that. It needs a grammar
  change: `data_parameter` takes no decorator today, so every such line is
  `# hypothetical syntax`.
- **The `->` clause** of a variant (cases 5 and 8) is not a marker, but it
  also reads payload names, left to right on the same line. Whether it may
  is unstated in 08 ([problem 9](#9-common-fields-per-value-data-and-decision-10s-two-outcomes)).

V1's scope has these observable effects in the cases: a payload member
shadows a module-level function of the same name inside the message (a
member named `path` or `format`); unnamed payloads cannot be interpolated
(R18, case 6); tooling must know that a decorator argument, unlike every
other, sees the next line's names (rename of a payload member must edit
the message); and `$self` is rejected (R7), while V2's hand-written
`to_string` that interpolates `$self` recurses at run time like any
`Display` would.

## Where Library Derivations Want The Same Access

The record gives payload scope to `@message` only. Library annotations are
ordinary metadata values built in module scope (14 Annotations,
Member Metadata), and a field default does not bind other fields
(`r[data.default.no-field-binding]`). These are the places in common Rust
derive usage where a library would have wanted the same access, recorded
as findings, not proposals:

```text
use dep.json
use dep.check
use dep.cli

@derive(json.Json)
pub data Profile:
    name: string
    @json.skip_if(is_blank)
    nickname: string
    @json.default(fn() -> string: "")
    display_name: string

fn is_blank(text: string) -> bool: text.length() == 0

@check.valid(fn(range: Window) -> bool: range.end >= range.start)
pub data Window:
    start: i64
    end: i64

@cli.command
pub data SearchArgs:
    @cli.help("number of worker threads")
    jobs: i64 = default_jobs()
```

- **serde `skip_serializing_if`.** serde takes a function path; hd takes a
  function value, so `@json.skip_if(is_blank)` works without member scope.
  `nickname.is_empty()` would read better, and a method value
  (`string.is_empty`) is deferred (07, P6).
- **Cross-field validation** (validator's `#[validate(schema(function =
  ...))]`, garde's `#[garde(custom(...))]`). A closure over the whole value
  works; member scope would let it read `end >= start`.
- **Computed defaults** (serde's `#[serde(default = "path")]`, or a
  default derived from another field). A function value covers the first
  (`@json.default(...)` above). For the second, hd's field defaults cannot
  see other fields, by rule, and a library annotation cannot either, so
  "the display name defaults to the name" needs a factory function.
- **CLI help that mentions a default** (clap's `default_value_t` appends
  `[default: 4]`). clap computes that from the default itself. In hd a
  library would read the field's default through the derivation API, not
  through the help string's scope.
- **Error messages in other libraries.** miette's `#[diagnostic(help(...))]`
  and `#[label("...")]` interpolate fields exactly as thiserror's
  `#[error]` does. A diagnostics library on hd could not offer that shape,
  since only `@message` gets the scope; it would take a function of the
  value, or ask the user to implement a trait.

## V1, V2a, And V2b Compared

The table lists consequences; it does not rank them.

| | V1 | V2a | V2b |
| --- | --- | --- | --- |
| Where the message lives | `@message` above each variant | `impl Display` `match`, separate from the declaration | same as V2a |
| Names a message can use | payload members, data fields, common fields, by a scope only this marker has | pattern bindings in the `match`, ordinary scope | same as V2a |
| New variant without a message | compiles, displays as the variant name | `Display` `match` is non-exhaustive: compile error (unless `_ =>`) | same as V2a |
| Unnamed payload in a message | impossible (R18) | fine (positional pattern) | fine |
| Message for a `Display`-only kind enum | not available (`@message` needs `@derive(Error)`, which makes the kind a cause) | hand-written, same as every `Display` | same as V2a |
| `Display` different from the message | impossible (E2) | the `Display` is the message | same |
| Markers, case 7 (12 variants) | 16 | 4 | 1 |
| Lines, case 7 | 34 | 43 | 40 |
| Transparent one-payload wrapper | `@transparent` | `@transparent` plus a forwarding arm | a forwarding arm |
| Transparent wrapper with extra fields (cargo, `io::Error`) | hand-write all impls | hand-write all impls | derive; forwarding arm |
| Transparent data type | hand-write | hand-write | derive |
| `find[Inner]` through a wrapper | `.None` (R10) | `.None` | found |
| `root_cause` when the payload has no cause | the wrapper | the wrapper | the payload |
| Printed chain and `report_of` | each message once | once, if the arm forwards | once, by the skip rule |
| `chain(error)` itself | no duplicates | no duplicates | wrapper and payload both present, same message |
| Hand-written printers (cargo) | see what `main` prints | see what `main` prints | must repeat the skip rule |
| Two real levels with the same text | both printed | both printed | merged |
| Message and marker can disagree | no | yes: `@transparent` with a non-forwarding arm loses the inner message | no marker to disagree with |
| Bounds of a generic `Display` | inferred from interpolations (M12 rule, R16) | written by the author | written by the author |
| Compiler work | `Display`, `Error`, `From`; message scope | `Error`, `From` | `Error`, `From` |
| Library work | none | none | skip rule in printer and `report_of` |
| Grammar change | payload-parameter decorators (decision 12) | same | same |
| Closest precedent | thiserror | thiserror without `#[error]`; hand `Display` | Go's `%w` with `errors.Unwrap`, printed without repeats |

## Problems, Ranked

Ranked by how many cases hit them and how silent the failure is. Each
lists the variants affected and candidate fixes as questions. None is
decided here.

### 1. Automatic `From` Has No Opt-Out

**Effect.** A one-payload variant with a unique `Error` payload always
gets `From`. A hand-written `impl From[DbError] for AppError` that maps
unique-key violations to `Conflict` is `overlapping-impl` against the
generated one, and nothing turns the generated one off (case 7). All
variants.

**Candidates.**

- **A.** Should a hand-written `From[P]` for the same enum suppress the
  generated one instead of overlapping?
- **B.** Should a marker (for example `@from(false)`) opt a variant out?
- **C.** Should `From` be generated only for `@from` variants, as in
  thiserror, giving up the inference?

### 2. Inference Changes When An Upstream Type Starts Implementing `Error`

**Effect.** Cause and `From` are inferred from "the member's type
implements `Error`". If a dependency's minor release adds `impl Error for
Span`, every downstream `@derive(Error)` variant that holds a `Span`
changes: a lone `Span` payload gains a `From` (which may overlap a
hand-written one), and a variant holding a `Span` and another error now
has two candidate causes, so it stops compiling until `@source` is
added. The downstream package did not change. All variants.

**Candidates.**

- **A.** Should a lint report, when a dependency upgrade is resolved,
  every derived `From` or cause whose inference changed?
- **B.** Should inference be limited to one-payload variants, with
  multi-member variants always marking `@source`?
- **C.** Is this accepted as the cost of inference, and documented in the
  package-evolution rules ([Packages](PACKAGES.md))?

### 3. Automatic Cause Has No Opt-Out

**Effect.** Every `Error`-typed member is a cause candidate. A kind enum
that derives `Error` to get messages (V1, cases 1, 4, 9) becomes the
cause of its wrapper, and the chain prints the same sentence twice.
Conversely, a cause inside a member's variant (serde_json's `Io` code,
naga's `BadNumber` in a wrapper) cannot be named, so the whole impl is
hand-written (cases 3 and 8). All variants; V1 is hit more often because
it is the only way to get messages.

**Candidates.**

- **A.** Should a marker say "not a cause" (for example `@source(false)`
  on a member)?
- **B.** Should `@derive(Error)` accept a hand-written `cause()` and
  generate only the rest (`Display` in V1, `From` in both)?
- **C.** For V1 only: should `@message` be allowed on an enum that derives
  `Display` without `Error` (a message-only derive)?

### 4. V2b's Duplicate-Skipping Lives Only In Printers

**Effect.** V2b keeps `chain` exact, so `find` and `root_cause` see every
part, and the standard printer and `report_of` skip a part whose message
equals the previous one. A hand-written printer (cargo's, case 2) walks
`chain` and prints the repeat unless it copies the rule. Two real levels
with the same text are merged (case 13). Each comparison formats both
parts. V2b only.

**Candidates.**

- **A.** Should `std.error` export the skipping walk (for example
  `display_chain(error) -> List[Error]`) so every printer uses one rule?
- **B.** Should the rule skip a part only when its parent is a wrapper
  (a one-payload variant, or a data type with one `Error` field) and the
  messages are equal, so two unrelated `Context` levels are never
  merged?
- **C.** Should V2b skip nothing and let wrappers choose: forward the
  message and return `.None` from a hand-written `cause()`, as Go code
  that wraps with `%v` does?

### 5. Adding A Variant Can Remove A `From`

**Effect.** A variant that is the only one with payload `P` gets `From[P]`
without a marker. Adding a second variant with payload `P` removes it,
with only a lint note, and every `?` that relied on it becomes
`invalid-result-propagation`, including in dependent packages (cases 6,
7, 11). All variants.

**Candidates.**

- **A.** Should the conflict be an error at the declaration until one
  variant is marked `@from` or the author opts out?
- **B.** Should `From` require `@from` (thiserror), so adding a variant
  never changes a conversion?
- **C.** Should the lint be promoted to a warning that names the `?`
  sites in the declaring package?

### 6. V1 Displays A Variant Without `@message` As Its Name

**Effect.** A new variant compiles and prints `Timeout` or `Db` to users
(case 7). A wrapper variant without a message prints its name and then the
cause. V1 only; in V2 the `Display` `match` fails to compile instead.

**Candidates.**

- **A.** Should a missing `@message` be an error, with `@message("Timeout")`
  for the rare case where the name is the message?
- **B.** Should a wrapper variant without `@message` default to its
  payload's message (implicit transparency)?
- **C.** Should the default stay, with a lint?

### 7. V2a's `@transparent` Is Not Checked Against The `Display` Arm

**Effect.** In V2a, `@transparent` makes `cause()` skip the payload, but
the author writes the message. An arm that does not forward
(`"invalid configuration"`) drops the payload's message from every
output (case 7). V2a only.

**Candidates.**

- **A.** Should the compiler require a transparent variant's arm to be
  exactly `payload.to_string()` or `"$payload"`?
- **B.** Should V2a generate the arms of transparent variants and let the
  author write the rest (a partial `Display`)?
- **C.** Should V2a be dropped in favor of V1 or V2b, since its marker's
  promise is no longer the compiler's to keep?

### 8. `ErrorReport` Repeats The Top Message

**Effect.** The STDLIB sketch sets `message` to `error.to_string()` and
`causes` to every part of `chain(error)`, which starts with the error
itself, so `causes[0]` equals `message` (case 14). Under V2b the skip rule
must also apply. All variants.

**Candidates.**

- **A.** Should `causes` start at `error.cause()`?
- **B.** Should `ErrorReport` hold one list of parts, with no separate
  `message`?

### 9. Common Fields: Per-Value Data And Decision 10's Two Outcomes

**Effect.** Shared enum data comes from each variant's `->` clause, so a
per-value span or kind is a payload the clause copies (cases 5 and 8).
Whether the clause may name the payload is unstated in 08; if it may not,
per-value common fields are impossible. A common field without a default
blocks automatic `From` even when the variant's `->` clause supplies it
(R12). Decision 10 states the consequence twice, once as an error and
once as a lint note. All variants.

**Candidates.**

- **A.** Should 08 state that a variant's `->` arguments see its payload
  names?
- **B.** Should promotion use the `->` clause when it computes every
  common field from constants and the one payload?
- **C.** Which of "error" and "lint note" is the intended outcome?

### 10. `.context` Builds Its Message On Success

**Effect.** `.context("reading rules from $path")` evaluates the
interpolation whether or not the result is an error (case 10). anyhow
offers `.with_context(|| ...)` for this. All variants.

**Candidates.**

- **A.** Should `std.error` add `with_context(fn() -> string)`?
- **B.** Should `context` take its message lazily by rule (a thunk
  parameter), keeping one name?

### 11. Tests Cannot Propagate Non-Error Errors

**Effect.** A test's implicit `Result[void, Error]` accepts only errors
that implement `Error`. `?` on `Result[T, string]` is
`invalid-result-propagation`, and no `From[string]` into the erased
`Error` can be written (case 12). All variants.

**Candidates.**

- **A.** Should `?` in a test block also accept any `E < Display`,
  wrapping it in a std message error?
- **B.** Should `std.error` provide a `Message` error and a
  `.to_error()` helper for `Result[T, string]`?

### 12. The Entry Point Fixes Exit Status And Format

**Effect.** `main` returning `.Err` exits with 1 and prints the chain in
one format. ripgrep (status 2, silent on a broken pipe) and cargo (101,
`Caused by:` blocks) must print and exit by hand, outside `main`'s
result (cases 1, 2, 13). All variants.

**Candidates.**

- **A.** Should a `std.error` trait (for example `ExitStatus`) let an
  error type choose its status?
- **B.** Is printing and calling a `std.process` exit the intended route
  for such tools?

### 13. An Erased Member Makes An Error Type Non-Boundary-Safe

**Effect.** reqwest's `source`, `io::Error`'s custom payload, and an
`Other(error: Error)` catch-all all hold an erased `Error`, so the type
cannot be a registered function's error (cases 4, 5, 7, 14). std's domain
enums therefore cannot copy these crates' shapes. All variants.

**Candidates.**

- **A.** Should the error-handling guide state that boundary error types
  hold only boundary-safe payloads, and show `report_of` for the rest?
- **B.** Should a boundary adapter accept a type whose only unsafe members
  are `Error` or `Error?`, encoding those as reports?

### 14. `find` Requires `AnyRef`

**Effect.** `find[T < AnyRef + Inspectable]` works for enums only while
enums are references. [Enum Semantics](ENUM_SEMANTICS.md) (deferred)
recommends values; then `find[FsError]` fails its bound, in every case
that tests an enum error (cases 1, 10, 11, 12). All variants.

**Candidates.**

- **A.** Should `std.error` ship `find_val[T]`, built on `downcast_val`?
- **B.** Should the enum decision state what happens to `find` and
  `downcast` on enums?

### 15. Generic Constructors As Mapping Functions

**Effect.** `result.map_err(TaskError.Failed)` needs an expected
monomorphic function type (08, `r[data.enum.fn-value.generic]`), and
`map_err[F, R]` does not supply `F`, so the closure form is needed
(case 7). All variants.

**Candidates.**

- **A.** Should `F` be inferred from the enclosing `?`'s target when
  `map_err` is its operand?
- **B.** Is the closure form accepted, and the diagnostic asked to
  suggest it?

### 16. No Non-Exhaustive Error Enums

**Effect.** grep-regex and globset mark their kind enums
`#[non_exhaustive]` so new kinds are minor releases. hd enums are closed,
so any new variant breaks every exhaustive `match` downstream (cases 1
and 9). All variants.

**Candidates.**

- **A.** Should public enums have a non-exhaustive form that requires a
  wildcard arm outside the declaring package?
- **B.** Is the private-field wrapper plus accessor methods the intended
  idiom for evolving error kinds?

### 17. Cyclic Chains

**Effect.** An error type with a `mut` field for its cause can be made to
point at itself or an ancestor; `chain`, `find`, `root_cause`, the entry
point, and `report_of` then loop (case 13). All variants.

**Candidates.**

- **A.** Should the chain helpers stop at a part already visited
  (reference identity)?
- **B.** Should they stop at a fixed depth and say so in the output?

Also seen, and already tracked: `?` silently picks the `@from` variant
among variants that share a payload type (R11, case 6); the bounds of the
generated impls (R16, case 7); and Rust's `{:?}` quoting has no hd
counterpart, so messages need helpers (case 1).

## Parse Log

Every `text` block above was extracted and checked with the chapter-02
reference parser (`spec/reference-parser/parser.ts`, `parseSource`) on
2026-09-27. Parsing checks syntax only: names such as `Span`, `IoError`,
`read_file`, and the helper functions are not resolved, nothing was
type-checked, and the prototype compiler implements none of this surface.
All 31 `text` blocks are listed; the 9 `rust` blocks were not parsed.

| Blocks | Result |
| --- | --- |
| Case 1: `RegexError` in V1, the derived-kind spelling, V2, and ripgrep's `main` | Parse. |
| Case 2: `ManifestError` with a forwarding `@message`, the hand-written wrappers and `HttpNotSuccessful`, the V2b wrappers, the layers and the printer | Parse. |
| Case 3: `JsonError` with its code enum, `Display`, `Error`, and `From` impls | Parse. |
| Case 4: `HttpError` in V1, its V2 `Display`, `is_timeout` and `without_url` | Parse. |
| Case 5: `IoError` hand-written and in V2b | Parse, including `->` clauses that name a payload. |
| Case 6: `RuleCoreError` in V1 and V2 | Parse. |
| Case 7: `AppError` and `TaskError[E]` in V1 and in V2 | `syntax-error` at the `Migration` line in each block, the `@source` payload parameter, marked `# hypothetical syntax`. With `@source ` removed, both parse. |
| Case 7: the generated `Error` and `From` impls | Parse. |
| Case 8: `WgslError` in V1 and V2, and `render` | Parse. |
| Case 9: `GlobError` in V1 and V2 | Parse. |
| Cases 10-14: `load_rules` and `exit_code`, the three layers and V2's `LoadError`, the test blocks, the repeated context, `load_tool` | Parse. |
| Library access: `Profile`, `Window`, `SearchArgs` | Parse. |

Reference-parser findings from this round:

- `@derive(Error)`, `@message(...)`, and the bare markers parse as
  decorators before a data type, an enum, and a variant line, including
  a variant with a `->` clause.
- `@source` before a payload parameter is a `syntax-error`, because
  `data_parameter` takes no decorator (decision 12 is not yet in 02).
  These lines are marked `# hypothetical syntax`; with the marker removed,
  every such block parses.
- A variant's `->` arguments that name its own payload (`-> WgslError(at)`)
  parse. The parser does not check scope, so this says nothing about
  problem 9.
