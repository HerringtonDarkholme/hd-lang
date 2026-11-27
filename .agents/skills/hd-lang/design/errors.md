# Errors: Absence, Failure And Panics

hd has no `null` and no exceptions. A missing value is an optional, a
failure the caller can handle is a `Result`, and a bug is a panic. This
page covers all three in depth. [language.md](../language.md) gives the
short version.

## When

| The situation | Write |
| --- | --- |
| a value may be absent: a lookup, a first element, a parse that has no failure story | `T?` |
| the call can fail and the caller decides what to do: a file, a request, user input | `Result[T, E]` with an `@error` enum for `E` |
| a caller broke the rules of the call: a negative amount, an empty list | `require` ([contracts](contracts.md)) |
| a state that cannot happen: a `match` arm after a check | `panic("why")` |
| a script or `main` that only reports and stops | `Result[void, *dyn Error]` and `.context("...")` |
| an error enum | `@error` on it, with a message per variant, even when it also derives `Eq` and `Debug` |
| a failure from another error type, crossing `?` | `@from` on the payload |
| a test must see a panic | `expect_panic=` ([testing](testing.md#cases-tests-it-and-it_each)) |

Expected failures are values. Do not panic for input from outside the
program.

## Optionals

`T?` is `Option[T]`: `.Some(value)` or `.None`. `?` on an optional
returns `.None` early from a function that returns an optional.

```hd
fn email_of(name: string) -> string?:
    if name == "Ada": .Some("ada@example.com") else: .None

fn domain_of(name: string) -> string?:
    email := email_of(name)?            # `?` returns `.None` early
    let .Some((_, domain)) = email.split_once("@") else: return .None
    .Some(domain)

let nickname: string? = .None
println(email_of("Grace").unwrap_or("no email"))
println("${debug(domain_of("Ada"))} ${nickname.is_none()}")
match email_of("Ada"):
    .Some(email) => println("found $email")
    .None => println("nobody")
```

```output
no email
Option.Some("example.com") true
found ada@example.com
```

## Results and `?`

`Result[T, E]` is `.Ok(value)` or `.Err(error)`. Postfix `?` unwraps an
`.Ok`, or returns the `.Err` from the current function.

```hd
enum CouponError:
    Unknown(code: string)

fn discount(code: string) -> Result[i32, CouponError]:
    if code == "SPRING10": .Ok(10) else: .Err(.Unknown(code))

fn checkout(cents: i32, code: string) -> Result[i32, CouponError]:
    percent := discount(code)?
    .Ok(cents * (100 - percent) / 100)

match checkout(5000, "SPRING10"):
    .Ok(due) => println("due: $due")
    .Err(.Unknown(code)) => println("unknown coupon: $code")
println(checkout(5000, "nope").unwrap_or(5000))
```

```output
due: 4500
5000
```

`?` works only in a function whose own return type is a matching
`Result` or optional. Elsewhere the error is `invalid-result-propagation`.
`CouponError` has no `@error` only to keep this sketch short. Give
every error enum an `@error`, as below.

## Taking a value out

| Call | Gives |
| --- | --- |
| `x.unwrap()` | the value, or a panic (`Result` errors require `Debug`) |
| `x.unwrap_or(fallback)` | the value, or the fallback |
| `x.unwrap_or_else(f)` | the value, or a fallback computed on failure |
| `x.expect("why it is present")` | the value, or a panic with that message |
| `x.or_else(f)` | this success, or an optional/result computed on failure |
| `x.map(fn(v): ...)` | the same wrapper around a changed value |
| `x.and_then(f)` | chains a step that itself returns an optional or result |
| `x.is_some()`, `x.is_none()`, `r.is_ok()`, `r.is_err()` | a `bool` |
| `r.ok()`, `r.err()` | the value or the error as an optional |
| `r.map_err(f)` | the same result with a changed error |

Use `unwrap` and `expect` only where a failure means a bug: in tests, or
after a check that makes it impossible. Write the reason in `expect`.

A `Result` or optional that is computed and not used is an error
(`discarded-must-use-value`). Handle it, or discard it on purpose with
`_ := expr`.

## Error enums

`@error` on an enum writes its `Display` and `Error` impls. Each variant
takes `@error("message")`. Payload members are in scope by name in the
message: write `$port` or `${port + 1}`. `@from` on a payload member
writes `impl From[...]`, so `?` converts that error into this one.

```hd
use std.num.ParseNumberError

@derive(Eq, Debug)
@error
enum ConfigError:
    @error("the port is missing")
    MissingPort
    @error("port is not a number")
    InvalidPort(@from error: ParseNumberError)
    @error("port $port is out of range")
    OutOfRange(port: i32)

fn parse_port(text: string) -> Result[i32, ConfigError]:
    if text == "":
        return .Err(.MissingPort)
    port := text.parse::[i32]()?        # a ParseNumberError becomes InvalidPort
    if port < 1 || port > 65535:
        return .Err(.OutOfRange(port))
    .Ok(port)

for text in ["8080", "", "eighty", "99999"]:
    match parse_port(text):
        .Ok(port) => println("port $port")
        .Err(error) => println("error: $error")
```

```output
port 8080
error: the port is missing
error: port is not a number
error: port 99999 is out of range
```

Message rules:

- A message is a plain interpolated string. Unnamed payload members are
  `$_0`, `$_1`. A variant with no message displays as its name.
- A name that is not a member is `unknown-name`, and `$self` is too.
- Every interpolated member must implement `Display`.

`@from` rules:

- It goes on the only payload member of a variant, and `?` then
  converts that type.
- Two `@from` members of one type in an enum are `overlapping-impl`.
- A `@from` member whose type is a bare type parameter is
  `invalid-error-marker`. Convert by hand with `map_err`.
- Without `@from`, convert by hand: `.map_err(fn(e): ConfigError.Other(e))`,
  or write `impl From[Other] for Mine` with `use std.convert.From`.

### Causes and transparent errors

`@source` marks a cause that is kept without a `From`. A variant has at
most one `@from` or `@source` member. `@error(transparent)` on a variant
or struct with one member shows that member's message and skips it in
the cause chain, which hides an inner type behind a stable one.

```hd
use std.error.report_of

@error("disk full")
struct DiskError:
    free: i64

@error("quota used")
struct QuotaError:
    used: i64

@error
enum SaveError:
    @error("cannot save $path")
    Write(path: string, @source error: DiskError)
    @error(transparent)
    Quota(@from error: QuotaError)

fn check_quota(used: i64) -> Result[void, QuotaError]:
    if used > 1000: .Err(QuotaError { used: used }) else: .Ok(())

fn save(path: string, free: i64, used: i64) -> Result[void, SaveError]:
    if free < 10:
        return .Err(.Write(path, DiskError { free: free }))
    check_quota(used)?                  # a QuotaError becomes Quota
    .Ok(())

for (free, used) in [(0, 0), (500, 2000), (500, 5)]:
    match save("a.txt", free, used):
        .Ok(_) => println("saved")
        .Err(error) => println(report_of(&error))
```

```output
cannot save a.txt
source: disk full
quota used
saved
```

## Any error, with context

`*dyn Error` holds any error, and `?` converts every std error and every
`@error` type into it. Use it where the caller only reports: a script, a
`main`, glue code. `.context("...")` wraps an error with a message, and
`message("text")` makes a one-off error with no type. `report_of` prints
the message, then one `source:` line per cause.

```hd
use std.error.{Error, message, report_of}

fn parse_port(text: string) -> Result[i32, *dyn Error]:
    if text == "":
        return .Err(message("empty port"))
    text.parse::[i32]().context("reading the port")

fn load(text: string) -> Result[i32, *dyn Error]:
    port := parse_port(text)?
    .Ok(port + 1)

for text in ["eighty", "", "80"]:
    match load(text):
        .Ok(port) => println("port: $port")
        .Err(error) => println(report_of(error))
```

```output
reading the port
source: invalid digit at position 0
empty port
port: 81
```

To look inside an any-error, call `error.chain()` (the error, then each
cause), `error.root_cause()` (the last one) or `error.find::[T]()` (the
first error of type `T`, as an optional). `find` needs an exact error type
and sees no error that a transparent wrapper hides. A report holds text
only: code cannot find the original error in it.

## Errors from `main`

`main` may return a `Result`. An `.Err` prints its report and exits with
status 1. End the success path with `.Ok(())`.

```hd
use std.fs.{FsError, FsRead, read_text}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ Console + FsRead:
    text := read_text!(Path("notes.txt"))?
    println("${text.lines().len()} lines")
    .Ok(())
```

To choose the status, return `ExitCode` from `std.process`:

```hd
use std.process.ExitCode

pub fn main() -> ExitCode $ Console:
    println("failing")
    ExitCode(3)
```

## Panics

`panic("message")` stops the program with a message and a stack, and the
exit status is 3. Use it for a bug, not for an expected failure. A panic
cannot be caught in code. Each panic has a category, which `expect_panic`
in a test names.

| Category | Raised by |
| --- | --- |
| `explicit-panic` | `panic(...)`, a failed `expect` or `unwrap`, a broken `require` or `ensure` |
| `assertion-failed` | a failed `assert`, `assert_equal` or snapshot |
| `index-out-of-bounds` | an index past the end of a list |
| `integer-overflow` | an arithmetic result that does not fit its type |
| `integer-division-by-zero` | `/` or `%` by zero |

Others are `invalid-shift`, `stack-exhausted`, `heap-exhausted`,
`borrow-conflict`, `time-limit` and the `suspension-*` family. A panic
prints as `panic: CATEGORY: message`, then the call stack.
