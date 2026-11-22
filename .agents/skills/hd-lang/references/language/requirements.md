# Requirements And Providers

Everything a function needs from outside itself is written in its
signature, in a **requirement row** after `$`. Printing is `Console`,
reading a file is `FsRead`, the time is `Clock`. A function cannot use
what its row does not name.

## Rows

```hd
use std.time.{Clock, now}

fn greet(name: string) -> void $ Console:
    println("hello, $name")

fn stamp(message: string) -> string $ Clock:
    "${now()}: $message"

fn announce(name: string) -> void $ Console + Clock:    # rows join with `+`
    greet(name)
    println(stamp("done"))

pub fn main() -> void $ Console + Clock:
    announce("Ada")
```

- A function's row must cover the row of everything it calls. Leaving one
  out is `missing-requirement`, and the message names what to add.
- A function with no `$` needs nothing: it is pure computation.
- `Console` needs no import. Every other capability trait is imported, as
  `use std.time.Clock`. The list is in [std/imports.md](../std/imports.md).
- A script (top-level statements, no `main`) may use every capability
  without writing a row.

## The host capabilities

`hd` supplies these to `main` when its row names them:

| Trait | Import from | Gives |
| --- | --- | --- |
| `Console` | prelude | `println`, `print`, `eprintln` |
| `ConsoleInput` | `std.console` | `read_line!`, `read_all!` |
| `Args`, `Env`, `Sys` | `std.host` | arguments, environment variables, system facts |
| `Clock` | `std.time` | `now`, `sleep!` |
| `Random` | `std.random` | random numbers |
| `FsRead`, `FsWrite` | `std.fs` | reading and writing files |
| `Process` | `std.process` | running other programs |
| `Http`, `Net` | `std.http`, `std.net` | requests and sockets |

In a package, `hd.toml` must also grant each one that `main` names
([cli/packages.md](../cli/packages.md)).

## Your own requirements

Any trait can be a requirement. `$.use(Trait)` fetches the current
provider, and `$.with(Trait=value):` supplies one for a block. A provider
is a shared object, so it is made with `box(...)`.

```hd
pub trait Mailer:
    fn send(&mut self, to: string, body: string) -> void

struct Outbox:
    sent: List[string]

impl Mailer for Outbox:
    fn send(&mut self, to: string, body: string) -> void:
        self.sent.push("$to: $body")

pub fn remind(users: List[string]) -> usize $ Mailer:
    mailer := $.use(Mailer)
    for user in users:
        mailer.send(user, "your invoice is due")
    users.len()

pub fn main() -> void $ Console:
    outbox := box(Outbox { sent: [] })
    $.with(Mailer=outbox):
        count := remind(["ada", "grace"])
        println("sent $count")
    println(outbox.sent.join("; "))
```

```output
sent 2
ada: your invoice is due; grace: your invoice is due
```

- `main` is where a program is wired: it builds the real providers and
  installs them, so its own row lists only host capabilities. A test
  installs fakes the same way ([testing/fakes.md](../testing/fakes.md)).
- Several providers go in one `$.with(Mailer=a, Repo=b):`.
- The provider must be a box: `$.with(Mailer=Outbox { ... })` with a plain
  value is a `type-mismatch`.
- A `pub` function's row may name only `pub` traits.
- A trait method's implementation has the same row as the trait declares.
  A provider that itself needs `Console` needs the trait method declared
  with `$ Console`.

## Callbacks with rows

A function type has a row too. A callback that prints is
`fn(T) -> void $ Console`. To accept a callback with any row, add a row
parameter `$ R` and pass it through.

```hd
fn each(items: List[i32], visit: fn(i32) -> void $ Console) -> void $ Console:
    for item in items:
        visit(item)

fn retry[T, $ R](times: usize, body: fn() -> T? $ R) -> T? $ R:
    for _ in 0..times:
        match body():
            .Some(value) => return .Some(value)
            .None => pass
    .None

pub fn main() -> void $ Console:
    each([1, 2], fn(item): println("item $item"))
    let attempts = 0
    answer := retry(3, fn() -> i32?:
        attempts += 1
        if attempts < 2: .None else: .Some(42)
    )
    println("${debug(answer)} after $attempts")
```

```output
item 1
item 2
Option.Some(42) after 2
```
