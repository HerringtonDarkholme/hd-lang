# Learn hd-lang in 10 Minutes

hd-lang is a statically typed, indentation-based language that compiles to
WebAssembly with values in linear memory and reference counting plus cycle
collection. It is designed for code that AI agents write and humans review:
types sit at the boundaries, dependencies are declared in signatures, and
suspension points are marked at every call.

This page is the syntax on one page, by example. Every `hd` block checks with
the compiler; `text` blocks are fragments that need the declarations around
them in a real program. Each topic links to its page of the
[tour](../tour/01-hello.md), where you run and change the code; the
[specification](../../spec/README.md) is the normative reference.

## Hello

A file can start with plain top-level statements. `#` starts a comment.

```hd
# hello.hd
println("hello, hd-lang")
```

Blocks open with a header that ends in `:`, followed by an indented body or a
same-line body. `$ Console` is a requirement row: the function may print.

```hd
fn greet(name: string) -> void $ Console:
    println("hello, $name")

fn shout(name: string) -> void $ Console:
    println(name.upper())

greet("Ada")
```

A program that reads input or files makes bang calls, such as `read_line!()`,
and only a `!` function may make them, so its entry point is `main!`
([tour: suspension](../tour/16-suspension.md)):

```text
use std.console.{ConsoleInput, read_line}

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput:
    let words: usize = 0
    while true:
        line := match read_line!()?:
            .Some(text) => text
            .None => break
        words = words + line.split_whitespace().len()
    $.use(Console).write!("words: $words\n")
```

## Bindings, Numbers, and Strings

`:=` binds a name that cannot be reassigned. `let` declares a variable that
can be ([tour: values and mut](../tour/02-values-and-mut.md)).

```hd
name := "Ada"
let attempts = +0
attempts = attempts + 1
```

Numbers have explicit widths: `i8` to `i64`, `u8` to `u64`, `usize` for
lengths and indices, `f32`, and `f64`. A plain integer literal takes its type
from its uses; with none, it is `usize`, or `i32` when written with a sign,
as `+5`. Conversions are calls, and none is implicit. Strings interpolate
`$name` and `${expression}`, are UTF-8 bytes, and slice by byte range
([tour: strings and numbers](../tour/03-strings-and-numbers.md)).

```text
use std.text.r

let big: i64 = 9000
narrow := i16(big)
mask := 0xFF_00
initial := 'A'
summary := "User ${user.name} has ${posts.len()} posts"
first := summary[0..4]
pattern := r"\d+ costs $5"
```

## Collections

`List[T]` and `Map[K, V]` come from the prelude, and tuples are built in.
Comprehensions put the clauses before `=>` and the produced value after it.
A list you will change is a `let mut` variable, and a list is a value, so
`b := a` copies it ([tour: collections](../tour/10-lists-and-maps.md)).

```hd
names := ["Ada", "Grace", "Linus"]
scores := { "Ada": 10, "Grace": 12 }
let (x, y) = (10, 20)
long_names := [for name in names if name.len() > 3 => name]
total := scores.get("Ada").unwrap_or(0)
let mut todos: List[string] = []
todos.push("buy milk")
```

## Functions

Parameters and results are typed, and the last expression is the value.
Parameters may have defaults, and calls may name arguments. Closures use the
same `fn` syntax, generic parameters go in square brackets, and a bound is
written with `<` ([tour: functions](../tour/03-functions.md),
[generics](../tour/10-generics.md)).

```hd
fn connect(host: string, port: i32 = 443) -> string:
    if host == "":
        return "no host"
    "$host:$port"

connect("localhost", port=8080)
inc := fn(x: i32) -> i32: x + 1

fn show[T < Display](value: T) -> string:
    value.to_string()
```

A pipe feeds a value into the next step, and `_` marks where it goes
([tour: pipes](../tour/08-pipes.md)): `cents |> discounted(_, 10) |> format`.

### Inlining and cold paths

A URL validator calls its tiny prefix test on every request, while malformed
input is rare. Inlining changes that code layout, never program behavior.
Import `inline` or `cold` from `std.annotation`. `@inline(.Always)` asks the
compiler to copy a copyable function at each direct call even past its normal
size and reference rules; an uncopyable function is an
`inline-always-uncopyable` error.
`@inline(.Never)` keeps a body out of its callers, and `@cold` additionally
routes each direct call through the function table. Bare `@inline` is
`@inline(.Auto)`.

```hd
use std.annotation.{cold, inline}

@inline(.Always)
fn has_scheme(text: string) -> bool:
    text.starts_with("https://") || text.starts_with("http://")

@cold
fn malformed_url(text: string) -> string:
    "invalid URL: $text"

fn validate_url(text: string) -> Result[string, string]:
    if has_scheme(text): .Ok(text) else: .Err(malformed_url(text))
```

## Control Flow

The whole list: `if`, `match`, `for`, `while`, `break`, `continue`, and
`else` on a loop. There is no `loop`; write `while true:`. `if`, `match`, and
loops with `else` are expressions, and a `match` must be exhaustive
([tour: control flow](../tour/07-expressions.md)).

```text
label := if score >= 90: "excellent" else: "passing"

found := for name in names:
    if name.starts_with("G"):
        break name
else:
    "nobody"
```

`defer:` registers cleanup that runs when the enclosing block exits.

## Data and Mutability

`struct` declares a record, which is a value: `b := a` copies it, and
changing one never changes the other. A field may have a default, and `...`
copies a value with changes. `let mut` marks a variable whose fields change
in place. Sharing is written: `&T` reads a value where it lives with no
copy, `&mut T` also writes it, and `&mut draft` lends a `let mut` variable
to a callee, whose writes the caller sees. `&mut User { ... }` makes a new
object that every holder of the reference shares
([tour: values and mut](../tour/02-values-and-mut.md),
[data](../tour/09-data.md)).

```hd
struct User:
    id: string
    email: string
    nickname: string? = .None

fn normalize(user: &mut User) -> void:
    user.email = user.email.trim().lower()

fn label(user: &User) -> string:
    "${user.id} <${user.email}>"

let mut draft = User { id: "u2", email: " Grace@Example.com " }
normalize(&mut draft)
renamed := User { ...draft, nickname: "grace" }
println(label(&renamed))
```

## Enums, Optionals, and Errors

An `enum` is a closed set of variants, which may carry fields. `.Variant`
names one where the type is known. `==` on an enum needs `@derive(Eq)`
([tour: enums](../tour/04-enums-and-match.md)).

```hd
enum Shape:
    Circle(radius: f64)
    Rect(width: f64, height: f64)

fn area(shape: Shape) -> f64:
    match shape:
        .Circle(radius) => 3.14159 * radius * radius
        .Rect(width, height) => width * height
```

`T?` is `Option[T]`: `.Some(value)` or `.None`. `Result[T, E]` is
`.Ok(value)` or `.Err(error)`. Postfix `?` unwraps, or returns the `.None` or
`.Err` from the current function. `@error` writes an error enum's messages
and conversions ([tour: option](../tour/05-option.md),
[errors](../tour/06-errors.md), [error context](../tour/15-error-context.md)).

```hd
@error
enum ConfigError:
    @error("config not found: $path")
    Missing(path: string)

fn read_config(path: string) -> Result[string, ConfigError]:
    .Err(ConfigError.Missing(path))

fn port(path: string) -> Result[string, ConfigError]:
    text := read_config(path)?
    .Ok(text.trim())
```

## Traits

A trait lists methods, and each implementation is explicit. A bound
dispatches statically; a trait value, `&dyn Trait` or `&mut dyn Trait`,
dispatches dynamically and is always written with its `&`. `@derive` writes
standard implementations ([tour: traits](../tour/11-traits.md),
[derive](../tour/18-derive.md)).

```text
trait Describe:
    fn describe(&self) -> string

impl Describe for User:
    fn describe(&self) -> string:
        self.email

@derive(Eq, Hash)
struct Point:
    x: i32
    y: i32
```

Typed derivation is ordinary library code over `std.structure`. Its `Field`,
`Variant`, `Members`, and `Key` values are read-only handles to per-type
metadata that lives for the whole program. Passing a handle copies the small
reference, not the member or variant metadata it names.

## Requirements and Suspension

A row after `$` lists the traits a function needs. `$.use` retrieves a
provider and `$.with` supplies one for a block, as a reference to a new
object (`&FixedClock { at: 42 }`), so a test installs a fake
([tour: requirements](../tour/12-requirements.md),
[least authority](../tour/17-least-authority.md)). A name ending in `!`
can suspend, and so does every call to it
([tour: suspension](../tour/16-suspension.md)).

```text
trait Clock:
    fn now(&self) -> i64

fn stamp(message: string) -> string $ Clock:
    "${$.use(Clock).now()}: $message"

$.with(Clock=&FixedClock { at: 42 }):
    println(stamp("ready"))
```

## Modules, Tests, and Packages

Each file is a module named by its path under `src/`, and declarations are
private unless marked `pub`. `use` roots are `pkg`, `std`, `dep`, `self`, and
`super` ([tour: modules](../tour/14-modules.md)). A file's tests live in a
`tests:` block ([tour: tests](../tour/13-tests.md)).

```text
use pkg.user.types.{User, UserId}

pub fn main!() -> Result[void, ConsoleError] $ Console:
    let mut console = $.use(Console)
    console.write!("starting\n")?
    .Ok(())

tests:
    it("stamps with the fixed clock"):
        $.with(Clock=&FixedClock { at: 1 }):
            assert_equal(stamp("go"), "1: go", reason="uses the provider")
```

After `hd add ../money`, another package's public names are `dep.money`
([tour: packages](../tour/14-packages.md),
[Packages And Dependencies](WORKING_WITH_DATA.md#packages-and-dependencies)):

```text
use dep.money.{of_cents, show}

pub fn main() -> void $ Console:
    println("total: ${show(of_cents(1250))}")
```

## The Prelude

These names need no `use` in any module ([Prelude](../../spec/lang/10-modules.md#prelude)):

| Kind | Names |
| --- | --- |
| Types | `bool`, `i8` to `i64`, `u8` to `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `never`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result` |
| Traits | `Display`, `Debug`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Iterator`, `Iterable` |
| Other types | `Ordering`, `Console`, `ConsoleError`, `Suspend` |
| Functions | `println`, `panic`, `debug`, `dbg` |
| Tests only | `it`, `assert`, `assert_equal` |

Everything else is imported: `From`, `Error`, `Inspectable`, `block_on`,
`Hasher`, `Poll`, `PollContext`, `Waker`, `ConsoleInput`, `read_line`, and
the `fs`, `time`, and `text` helpers. `@derive`, `@error`, and the other intrinsic annotations need no
import.

## Common String Methods

A string is UTF-8 bytes, so `len()` counts bytes. The rest of the methods
are in the [text module](../../spec/std/text.md).

| Method | Result |
| --- | --- |
| `s.len()` | byte count, `usize` |
| `s.is_empty()` | `bool` |
| `s.chars()` | an iterator of `char`; `for c in s.chars()` |
| `s.lines()` | `List[string]`, without line endings |
| `s.split(",")` | `List[string]`, keeping empty pieces |
| `s.split_whitespace()` | `List[string]` of the words |
| `s.split_once("=")` | `(string, string)?` around the first match |
| `s.trim()`, `s.trim_start()`, `s.trim_end()` | `string` |
| `s.lower()`, `s.upper()` | `string` |
| `s.contains(t)`, `s.starts_with(t)`, `s.ends_with(t)` | `bool` |
| `s.find(t)` | byte offset, `usize?` |
| `s.replace(old, new)` | `string` |
| `s.strip_prefix(t)`, `s.strip_suffix(t)` | the rest, `string?` |
| `s.count(t)` | non-overlapping matches, `usize` |
| `s.pad_start(width)`, `s.pad_end(width)` | `string` |
| `s.repeat(n)` | `string` |

To build text in a loop, use a `StringBuilder` or `join`. Each copies
every byte once. `out = out + piece` copies all of `out` again on every
pass, as Go's `s += t` does.

```hd
use std.text.StringBuilder

fn csv(cells: List[string]) -> string:
    out := StringBuilder::new()
    for cell in cells:
        out.push(cell)
        out.push(";")
    out.build()
```

`cells.join(";")` builds the same text without the trailing separator.

## Format And Lint

`hd fmt` has one canonical style, like `gofmt`: four-space indentation, a
100-column width, and no style settings. It formats a package in place;
`hd fmt --check` writes nothing and lists files that differ.

`hd check`, `hd build`, `hd test`, the REPL, and the playground report errors
only. Run `hd lint` when you want optional advice. Lints default to their
registered level and can be selected explicitly, for example
`hd lint --deny redundant-let-mut` or `hd lint --allow unused-local-binding`.

## Where Next

- The [tour](../tour/01-hello.md) runs each topic in the browser.
- [Working With Data](WORKING_WITH_DATA.md) shows everyday jobs: JSON
  config, files, [reusable timeout and retry policies](WORKING_WITH_DATA.md#composing-reusable-policies),
  wiring a program, and packages.
- The [Language Overview](OVERVIEW.md) covers the purpose and priorities of
  the design.
- The [specification](../../spec/README.md) is the precise reference, starting
  with the [Type System](../../spec/lang/04-type-system.md) and
  [Requirements and Suspension](../../spec/lang/11-requirements-and-suspension.md).
