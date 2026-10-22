# Error

Status: standard library specification draft.

This chapter defines the part of `std.error` that `lib/std` writes in
ordinary hd over the language tier:

- the inherent methods `chain`, `root_cause`, and `find` of `&dyn Error`,
  which list and search an error's causes;
- the type `ErrorReport` and the helper `report_of`, which snapshot an
  error's message and its causes' messages;
- the function `message`, which makes an error from a text;
- the method `context` of `Result[T, E]`, which wraps errors with a message;
- which std error types implement `Error`.

The language tier keeps what the compiler knows by name:

| Item | Why it stays in the language tier |
| --- | --- |
| the trait `Error`, its member `cause`, and the `impl dyn Error:` block that holds `chain`, `root_cause`, and `find` | [Error Trait](../lang/09-traits.md#error-trait); the compiler generates implementations of it |
| `@error`, `@from`, `@source`, and transparent errors | [Error Derivation](../lang/14-annotations.md#error-derivation), a compiler intrinsic that defines `cause` ([Error Causes](../lang/14-annotations.md#error-causes)) |
| the erased `&dyn Error` and `?` into it | [Erased Errors](../lang/09-traits.md#erased-errors) and [Propagation](../lang/05-expressions.md#propagation) |
| what an entry point prints for an `.Err` | [Entry Results](../lang/10-modules.md#entry-results), which the host follows |
| which values cross a registered boundary | [Boundary-Safe Values](../lang/10-modules.md#boundary-safe-values) |

## Cause Chain

`chain` lists an error, then its cause, then that cause's cause. It is an
inherent method of `&dyn Error`:

```text
use std.error.Error

@error("disk full")
struct DiskError:
    free: i64

@error
enum SaveError:
    @error("cannot save $path")
    Write(path: string, @source error: DiskError)

fn messages(failure: SaveError) -> List[string]:
    let error: &dyn Error = &failure
    error.chain().map(fn(error: &dyn Error) -> string: error.to_string())
```

For `SaveError.Write("notes.txt", DiskError { free: 0 })`, `messages`
returns `["cannot save notes.txt", "disk full"]`.

1. r[std-error.chain.reference-method] `std.error` declares `pub fn chain(&self) -> List[&dyn Error]` in its `impl dyn Error:` block, so `e.chain()` lists the causes of an `&dyn Error` reference `e`.
2. r[std-error.chain.method-only] `std.error` exports no free `chain` function.
3. r[std-error.chain.method-order] `error.chain()` returns `error` first, then the error that its `cause` returns, then that error's cause, and so on.
4. r[std-error.chain.end] The list ends with the first error whose `cause` returns `.None`.

> **Note.** The list is never empty. A transparent error's `cause` skips
> its member, as [`annot.error.transparent.cause`](../lang/14-annotations.md#r-annot.error.transparent.cause)
> states, so `chain` never lists that member itself.

> **Note.** `chain` stops only at `.None`. A hand-written `cause` that
> leads back to an earlier error makes `chain` run forever.

> **Why.** [Entry Results](../lang/10-modules.md#entry-results) prints an
> error and then each cause that `chain` yields after it. One list that
> starts with the error itself serves both that rule and code that walks
> the causes.

## Root Cause And Find

The `&dyn Error` methods `root_cause` and `find` walk the list that `chain`
returns:

```text
use std.error.Error

@error("disk full")
struct DiskError:
    free: i64

@error
enum SaveError:
    @error("cannot save $path")
    Write(path: string, @source error: DiskError)

fn innermost(failure: &dyn Error) -> string:
    failure.root_cause().to_string()

fn free_space(failure: &dyn Error) -> i64?:
    match failure.find::[DiskError]():
        .Some(disk) => .Some(disk.free)
        .None => .None
```

For `SaveError.Write("notes.txt", DiskError { free: 0 })`, `innermost`
returns `"disk full"`, and `free_space` returns `.Some(0)`.

1. r[std-error.root-cause.method-chain] `e.root_cause()` returns the last shared error reference of `e.chain()`, including `e` itself when it has no cause.
2. r[std-error.dyn-reference-receiver.reference-values] `chain`, `root_cause`, and `find` are inherent methods of `&dyn Error`, by [`trait.error.dyn-methods.reference-values`](../lang/09-traits.md#r-trait.error.dyn-methods.reference-values). Their receivers have type `&dyn Error`; a concrete error reference converts before the call.
3. r[std-error.find.method-chain-first] `e.find::[T]()` returns `.Some` of the first error of `e.chain()`, in order, whose recorded type is `T`, and `.None` when no error does.
4. r[std-error.find.copied-value] The found error is copied through the reference returned by `downcast_ref::[T]()`.
5. r[std-error.find.struct-copy] For a struct target, the returned value is an independent copy of that struct.
6. r[std-error.find.concrete-exact] The target is a concrete error type and matches exactly, without a supertrait search.
7. r[std-error.find.bound] `T` must implement `Error`, so a target such as `i32` is an error. Error: `unsatisfied-trait-bound`.

```text
use std.error.Error

fn code(failure: &dyn Error) -> i32?:
    failure.find::[i32]()  # error: unsatisfied-trait-bound
```

> **Why.** Every error in a chain implements `Error`, so a target that
> does not could never match. The bound turns that mistake into a compile
> error, as Rust's `downcast_ref` bound on `Error` does.

## Error Reports

An `ErrorReport` is a boundary-safe snapshot of an error's message and the
messages of its causes:

```text
use std.error.{ErrorReport, report_of}

@error("disk full")
struct DiskError:
    free: i64

@error
enum SyncError:
    @error("sync failed")
    Save(@source error: DiskError)

fn summary() -> string:
    report := report_of(SyncError.Save(DiskError { free: 0 }))
    report.to_string()  # "sync failed\ncaused by: disk full"
```

1. r[std-error.report.decl-struct] `std.error` declares the struct `ErrorReport`, a value with the public fields `message: string` and `causes: List[string]`, and no other fields. Copying a report copies both fields.
2. r[std-error.report.import] Code imports both names, as in `use std.error.{ErrorReport, report_of}`.
3. r[std-error.report.boundary-safe] Both fields are boundary-safe, so `ErrorReport` is boundary-safe by [`module.boundary.allowed`](../lang/10-modules.md#r-module.boundary.allowed).
4. r[std-error.report.of.reference-values] `std.error` declares `pub fn report_of(error: &dyn Error) -> ErrorReport`.
5. r[std-error.report.message] The report's `message` is the `Display` text of `error`.
6. r[std-error.report.cause-texts] Its `causes` holds the `Display` text of each successive cause, starting with `error.cause()`.
7. r[std-error.report.display] `ErrorReport` implements `Display`. Its text is `message`, then, for each element of `causes`, a line feed, `caused by: `, and that element.
8. r[std-error.report.entry-text] So for an error `e` whose type implements `Error`, `report_of(e).to_string()` holds the lines that [Entry Results](../lang/10-modules.md#entry-results) prints for `.Err(e)`, joined by line feeds.

> **Note.** A report holds only text. It keeps no error value, so code
> cannot downcast it back to the error it came from.

> **Why.** An erased `&dyn Error` never crosses a registered boundary
> ([`module.boundary.erased-error`](../lang/10-modules.md#r-module.boundary.erased-error)).
> A report keeps the message and the causes apart, so a receiver can show
> the first line alone or the whole chain.

## One-Off Errors

`message` makes an error from a text, as Go's `errors.New` and Rust's
`anyhow!` do, so a script that wants to fail needs no error type:

```text
use std.error.{Error, message}

fn parse_port(text: string) -> Result[i32, &dyn Error]:
    if text == "":
        return .Err(message("empty port"))
    .Ok(80)
```

For `parse_port("")`, the error displays as `empty port`.

1. r[std-error.message.reference] `std.error` declares `pub fn message(text: string) -> &dyn Error`. Code imports it, as in `use std.error.message`.
2. r[std-error.message.reference-struct] The returned reference points to a private struct, whose type code cannot name or match on.
3. r[std-error.message.display] The error displays as `text`, unchanged.
4. r[std-error.message.cause] Its `cause` returns `.None`, so `chain` lists only that error.
5. r[std-error.message.reference-result] `.Err(message(text))` is a valid result of a function returning `Result[T, &dyn Error]`.

> **Why.** An ad hoc failure otherwise needs a struct, a `Display`
> impl, and an `Error` impl. The text is the whole error, as in Go.

## Error Context

`context` adds a message to an error on its way up, as Rust's
`anyhow::Context` does:

```text
use std.error.{Error, report_of}

@error("not found: $path")
struct NotFound:
    path: string

fn read(path: string) -> Result[string, NotFound]:
    .Err(NotFound { path: path })

fn load() -> Result[string, &dyn Error]:
    text := read("app.toml").context("loading config")?
    .Ok(text)

fn summary() -> string:
    match load():
        .Ok(text) => text
        .Err(failure) => report_of(failure).to_string()  # "loading config\ncaused by: not found: app.toml"
```

1. r[std-error.context.reference-result] `std.error` declares the inherent method `pub fn context(self, message: string) -> Result[T, &dyn Error]` on `Result[T, E]` for every `E < Error`.
2. r[std-error.context.no-use] Calling `context` needs no `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
3. r[std-error.context.ok] `.Ok(value).context(message)` returns `.Ok(value)`.
4. r[std-error.context.erased-wrapper] `.Err(error).context(message)` returns `.Err` of a shared reference to a wrapper containing `&error` and that message.
5. r[std-error.context.private-wrapper] The wrapper is a private struct with private fields, whose type code outside `std.error` cannot name.
6. r[std-error.context.wrapper-traits] The wrapper implements `Display` and `Error`.
7. r[std-error.context.wrapper-display] The wrapper displays as its message.
8. r[std-error.context.reference-cause] Its `cause` returns `.Some` of the shared reference to the wrapped error.
9. r[std-error.context.method-chain] `error.chain()` lists the wrapper first, then the wrapped error and its causes.
10. r[std-error.context.report-chain] `report_of` gives the context message, then one `caused by:` line per cause.

> **Why.** Code that propagates with `?` often knows what it was doing
> but not why the call failed. `context` records the first, keeps the
> second as the cause, and leaves `chain` and `report_of` unchanged.

## Standard Error Types

Every std error type implements `Error`, so `?` and `context` accept it:

```text
use std.error.Error

fn count(text: string) -> Result[i32, &dyn Error]:
    value := text.parse::[i32]()?
    .Ok(value)

fn width(text: string) -> Result[i32, &dyn Error]:
    text.parse::[i32]().context("reading the width")
```

| Type | Module |
| --- | --- |
| `ParseNumberError` | `std.num` |
| `Utf8Error` | `std.text` |
| `ParseBoolError` | `std.text` |
| `ParseCharError` | `std.text` |
| `ConsoleError` | `std.console` |
| `DecodeError` | `std.encoding` |
| `FsError` | `std.fs` |
| `ProcessError` | `std.process` |
| `JsonError` | `std.json` |
| `ParseTimeError` | `std.time` |
| `CliError` | `std.cli` |
| `HostError` | `std.host` |
| `RegexError` | `std.regex` |
| `NetError` | `std.net` |

1. r[std-error.std-types.impl.reference-values] Each type in the table above implements `Error`. So `?` converts it into a `Result[T, &dyn Error]`, by [`trait.error.result.reference-values`](../lang/09-traits.md#r-trait.error.result.reference-values), and `context` accepts it.
2. r[std-error.std-types.cause] Their `cause` is the default `.None`.
3. r[std-error.std-types.display] Each keeps the `Display` text its chapter defines.
4. r[std-error.std-types.string.reference-values] The language type `string` implements `Error`. Its `Display` text is the string itself, and its `cause` is the default `.None`. So `.Err("disk full")` is a valid `Result[T, string]` for a bound `E < Error`, and `?` can propagate it into `Result[T, &dyn Error]`.
5. r[std-error.std-types.new] A std error type added later implements `Error` too, and joins this table.

> **Why.** An application error type is the erased `&dyn Error` in most
> scripts. A std error that cannot reach it forces a wrapper type at
> every call, which Rust's `std::io::Error` and Go's `error` never need.

See also: [Error Trait](../lang/09-traits.md#error-trait),
[Error Derivation](../lang/14-annotations.md#error-derivation),
[Entry Results](../lang/10-modules.md#entry-results),
[Result](result.md).
