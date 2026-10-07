# Error

Status: standard library specification draft.

This chapter defines the part of `std.error` that `lib/std` writes in
ordinary hd over the language tier:

- the inherent methods `chain`, `root_cause`, and `find` of `dyn Error`,
  and the free function `chain`, which list and search an error's causes;
- the type `ErrorReport` and the helper `report_of`, which snapshot an
  error's message and its causes' messages;
- the method `context` of `Result[T, E]` and the type `ContextError` it
  wraps an error in;
- which std error types implement `Error`.

The language tier keeps what the compiler knows by name:

| Item | Why it stays in the language tier |
| --- | --- |
| the trait `Error`, its member `cause`, and the `impl dyn Error:` block that holds `chain`, `root_cause`, and `find` | [Error Trait](../lang/09-traits.md#error-trait); the compiler generates implementations of it |
| `@error`, `@from`, `@source`, and transparent errors | [Error Derivation](../lang/14-annotations.md#error-derivation), a compiler intrinsic that defines `cause` ([Error Causes](../lang/14-annotations.md#error-causes)) |
| the erased `dyn Error` and `?` into it | [Erased Errors](../lang/09-traits.md#erased-errors) and [Propagation](../lang/05-expressions.md#propagation) |
| what an entry point prints for an `.Err` | [Entry Results](../lang/10-modules.md#entry-results), which the host follows |
| which values cross a registered boundary | [Boundary-Safe Values](../lang/10-modules.md#boundary-safe-values) |

## Cause Chain

`chain` lists an error, then its cause, then that cause's cause. It is an
inherent method of `dyn Error`, and also a free function:

```text
use std.error.{Error, chain}

@error("disk full")
data DiskError:
    free: i64

@error
enum SaveError:
    @error("cannot save $path")
    Write(path: string, @source error: DiskError)

fn messages(failure: SaveError) -> List[string]:
    chain(failure).map(fn(error: dyn Error) -> string: error.to_string())
```

For `SaveError.Write("notes.txt", DiskError { free: 0 })`, `messages`
returns `["cannot save notes.txt", "disk full"]`.

1. r[std-error.chain.method] `std.error` declares `pub fn chain(self) -> List[dyn Error]` in its `impl dyn Error:` block, so `e.chain()` lists the causes of a `dyn Error` value `e`.
2. r[std-error.chain.decl] `std.error` also declares the free function `pub fn chain(error: dyn Error) -> List[dyn Error]`, which returns `error.chain()`. Code imports it, as in `use std.error.chain`.
3. r[std-error.chain.order] `chain(error)` returns `error` first, then the error that its `cause` returns, then that error's cause, and so on.
4. r[std-error.chain.end] The list ends with the first error whose `cause` returns `.None`.
5. r[std-error.chain.argument] The parameter has the erased type `dyn Error`, so any value whose type implements `Error` may be passed, as [Erased Errors](../lang/09-traits.md#erased-errors) allows.

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

The `dyn Error` methods `root_cause` and `find` walk the list that `chain`
returns:

```text
use std.error.Error

@error("disk full")
data DiskError:
    free: i64

@error
enum SaveError:
    @error("cannot save $path")
    Write(path: string, @source error: DiskError)

fn innermost(failure: dyn Error) -> string:
    failure.root_cause().to_string()

fn free_space(failure: dyn Error) -> i64?:
    match failure.find::[DiskError]():
        .Some(disk) => .Some(disk.free)
        .None => .None
```

For `SaveError.Write("notes.txt", DiskError { free: 0 })`, `innermost`
returns `"disk full"`, and `free_space` returns `.Some(0)`.

1. r[std-error.root-cause] `e.root_cause()` returns the last error of `chain(e)`, so an error without a cause is its own root cause.
2. r[std-error.dyn-receiver] `chain`, `root_cause`, and `find` are inherent methods of `dyn Error`, by [`trait.error.dyn-methods`](../lang/09-traits.md#r-trait.error.dyn-methods). A concrete error converts to `dyn Error` before the call.
3. r[std-error.find.first] `e.find::[T]()` returns `.Some` of the first error of `chain(e)`, in order, whose recorded type is `T`, and `.None` when no error does.
4. r[std-error.find.value] The found error is recovered as `downcast_val::[T]` recovers it, so it is readonly. An enum error comes back as an equal value.
5. r[std-error.find.reference] When `T` implements `AnyRef`, as a data error type does, the found error is the same reference that the chain holds, by [`trait.downcast.same-reference`](../lang/09-traits.md#r-trait.downcast.same-reference).
6. r[std-error.find.exact] Types match exactly, by [`trait.downcast.exact`](../lang/09-traits.md#r-trait.downcast.exact). No supertrait search takes place, so `e.find::[dyn Error]()` returns `.None`.
7. r[std-error.find.bound] `T` must implement `Error`, so a target such as `i32` is an error. Error: `unsatisfied-trait-bound`.

```text
use std.error.Error

fn code(failure: dyn Error) -> i32?:
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
data DiskError:
    free: i64

@error
enum SyncError:
    @error("sync failed")
    Save(@source error: DiskError)

fn summary() -> string:
    report := report_of(SyncError.Save(DiskError { free: 0 }))
    report.to_string()  # "sync failed\ncaused by: disk full"
```

1. r[std-error.report.decl] `std.error` declares the data type `ErrorReport` with the public fields `message: string` and `causes: List[string]`, and no other fields.
2. r[std-error.report.import] Code imports both names, as in `use std.error.{ErrorReport, report_of}`.
3. r[std-error.report.boundary-safe] Both fields are boundary-safe, so `ErrorReport` is boundary-safe by [`module.boundary.allowed`](../lang/10-modules.md#r-module.boundary.allowed).
4. r[std-error.report.of] `std.error` declares `pub fn report_of(error: dyn Error) -> ErrorReport`.
5. r[std-error.report.message] The report's `message` is the `Display` text of `error`.
6. r[std-error.report.causes] Its `causes` holds the `Display` text of each error that `chain(error)` returns after the first, in the same order.
7. r[std-error.report.display] `ErrorReport` implements `Display`. Its text is `message`, then, for each element of `causes`, a line feed, `caused by: `, and that element.
8. r[std-error.report.entry-text] So for an error `e` whose type implements `Error`, `report_of(e).to_string()` holds the lines that [Entry Results](../lang/10-modules.md#entry-results) prints for `.Err(e)`, joined by line feeds.

> **Note.** A report holds only text. It keeps no error value, so code
> cannot downcast it back to the error it came from.

> **Why.** An erased `Error` never crosses a registered boundary
> ([`module.boundary.erased-error`](../lang/10-modules.md#r-module.boundary.erased-error)).
> A report keeps the message and the causes apart, so a receiver can show
> the first line alone or the whole chain.

## Error Context

`context` adds a message to an error on its way up, as Rust's
`anyhow::Context` does:

```text
use std.error.{Error, report_of}

@error("not found: $path")
data NotFound:
    path: string

fn read(path: string) -> Result[string, NotFound]:
    .Err(NotFound { path: path })

fn load() -> Result[string, dyn Error]:
    text := read("app.toml").context("loading config")?
    .Ok(text)

fn summary() -> string:
    match load():
        .Ok(text) => text
        .Err(failure) => report_of(failure).to_string()  # "loading config\ncaused by: not found: app.toml"
```

1. r[std-error.context.decl] `std.error` declares the inherent method `pub fn context(self, message: string) -> Result[T, dyn Error]` on `Result[T, E]` for every `E < Error`.
2. r[std-error.context.no-use] Calling `context` needs no `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
3. r[std-error.context.ok] `.Ok(value).context(message)` returns `.Ok(value)`.
4. r[std-error.context.err] `.Err(error).context(message)` returns `.Err` of a `ContextError` whose message is `message` and whose cause is `error`.
5. r[std-error.context.type] `std.error` declares the data type `ContextError`, with private fields, which implements `Display` and `Error`. Code imports it, as in `use std.error.ContextError`.
6. r[std-error.context.display] A `ContextError` displays as its message.
7. r[std-error.context.cause] Its `cause` returns `.Some` of the wrapped error.
8. r[std-error.context.chain] So `chain` lists the `ContextError` first, then the wrapped error and its causes, and `report_of` gives the message, then one `caused by:` line per cause.
9. r[std-error.context.debug] `ContextError` implements `Debug`. It calls `out.debug_struct("ContextError")`, then `.field("message", m)` and `.field("cause", c)`, then `.finish()`.
10. r[std-error.context.debug.fields] There `m` is its message, and `c` is the `Display` text of the wrapped error, as a `string`.

> **Note.** In the compact layout, the `ContextError` of the example
> above shows as
> `ContextError { message: "loading config", cause: "not found: app.toml" }`.
> Like all `debug` text, that layout is not portable.

> **Why.** The wrapped error is an erased `Error`, which has no `Debug`.
> Its `Display` text is the one description every cause has, so a test
> that compares a `ContextError` can still show both parts.

> **Why.** Code that propagates with `?` often knows what it was doing
> but not why the call failed. `context` records the first, keeps the
> second as the cause, and leaves `chain` and `report_of` unchanged.

## Standard Error Types

Every std error type implements `Error`, so `?` and `context` accept it:

```text
use std.error.Error
use std.num.parse_i32

fn count(text: string) -> Result[i32, dyn Error]:
    value := parse_i32(text)?
    .Ok(value)

fn width(text: string) -> Result[i32, dyn Error]:
    parse_i32(text).context("reading the width")
```

| Type | Module |
| --- | --- |
| `ParseNumberError` | `std.num` |
| `Utf8Error` | `std.text` |
| `ConsoleError` | `std.console` |
| `DecodeError` | `std.encoding` |
| `FsError` | `std.fs` |
| `ProcessError` | `std.process` |
| `JsonError` | `std.json` |
| `TimeParseError` | `std.time` |
| `CliError` | `std.cli` |
| `RegexError` | `std.regex` |
| `ResourceError[E]`, when `E < Error` | `std.resource` |

1. r[std-error.std-types.impl] Each type in the table above implements `Error`. So `?` converts it into a `Result[T, dyn Error]`, by [`trait.error.result`](../lang/09-traits.md#r-trait.error.result), and `context` accepts it.
2. r[std-error.std-types.cause] Their `cause` is the default `.None`.
3. r[std-error.std-types.display] Each keeps the `Display` text its chapter defines.
4. r[std-error.std-types.resource] `ResourceError[E]` implements `Display` when `E < Display`. `Operation(error)` displays as `error` does, and `Disposed` as `resource disposed`.
5. r[std-error.std-types.new] A std error type added later implements `Error` too, and joins this table.

> **Why.** An application error type is the erased `Error` in most
> scripts. A std error that cannot reach it forces a wrapper type at
> every call, which Rust's `std::io::Error` and Go's `error` never need.

> **Why.** `ResourceError.Operation` shows its error's text and has no
> cause, so a report does not print the same text twice.

See also: [Error Trait](../lang/09-traits.md#error-trait),
[Error Derivation](../lang/14-annotations.md#error-derivation),
[Entry Results](../lang/10-modules.md#entry-results),
[Result](result.md).
