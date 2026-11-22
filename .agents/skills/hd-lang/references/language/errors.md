# Absence And Errors

hd has no `null` and no exceptions. A value that may be missing is an
optional, and a call that may fail returns a `Result`.

## Optionals

`T?` is `Option[T]`: `.Some(value)` or `.None`.

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

## Results

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

`?` works only in a function whose own return type is a matching `Result`
or optional. Elsewhere the error is `invalid-result-propagation`.

## Taking a value out

| Call | Gives |
| --- | --- |
| `x.unwrap()` | the value, or a panic (`Result` errors require `Debug`) |
| `x.unwrap_or_else(f)` | the value, or a fallback computed on failure |
| `x.or_else(f)` | this success, or an optional/result computed on failure |
| `x.unwrap_or(fallback)` | the value, or the fallback |
| `x.expect("why it is present")` | the value, or a panic with that message |
| `x.map(fn(v): ...)` | the same wrapper around a changed value |
| `x.and_then(f)` | chains a step that itself returns an optional or result |
| `x.is_some()`, `x.is_none()`, `r.is_ok()`, `r.is_err()` | a `bool` |
| `r.ok()`, `r.err()` | the value or the error as an optional |
| `r.map_err(f)` | the same result with a changed error |

`unwrap()` panics when there is no successful value. A `Result` or optional computed and not
used is an error (`discarded-must-use-value`): handle it, or discard it
with `_ := expr`.

## Error enums

`@error` on an enum writes its messages, and `@from` on a payload lets `?`
convert that error into this one.

```hd
use std.num.ParseNumberError

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

Without `@from`, convert by hand: `.map_err(fn(e): ConfigError.Other(e))`,
or write `impl From[Other] for Mine` with `use std.convert.From`.

## Any error, with context

`*dyn Error` holds any error. `.context("...")` wraps one with a message,
and `report_of` prints the chain.

```hd
use std.error.{Error, report_of}

fn parse_port(text: string) -> Result[i32, *dyn Error]:
    text.parse::[i32]().context("reading the port")

fn load(text: string) -> Result[i32, *dyn Error]:
    port := parse_port(text)?
    .Ok(port + 1)

match load("eighty"):
    .Ok(port) => println("port: $port")
    .Err(error) => println(report_of(error))
```

```output
reading the port
source: invalid digit at position 0
```

## Errors from `main`

`main` may return a `Result`. An `.Err` prints the error and exits with a
failing status. End the success path with `.Ok(())`.

```hd
use std.fs.{FsError, FsRead, read_text}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ Console + FsRead:
    text := read_text!(Path("notes.txt"))?
    println("${text.lines().len()} lines")
    .Ok(())
```

To choose the status yourself, return `ExitCode` from `std.process`:
`pub fn main() -> ExitCode $ Console:` ending in `ExitCode(0)`.

## Panics

`panic("message")` stops the program with a message and a stack. Use it
for a bug, not for an expected failure. An index out of range, an integer
overflow, a division by zero and a failed `expect` also panic.
