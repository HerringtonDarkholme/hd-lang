# Effects: Requirements And Providers

An effect is something a function needs from outside itself: the console,
the clock, a payment gateway. hd writes each one in the function's
signature, in a **requirement row** after `$`. Callers supply an
implementation, a **provider**, with `$.with`. Tests supply a fake the
same way. This page covers rows, your own effect traits, providers,
callbacks, and what should not be an effect.

## When

| You are about to | Do |
| --- | --- |
| print, read a file, ask the time, call the network | name the host capability in the row: `$ Console + FsRead + Clock` |
| call a service you will want to fake in tests | declare a trait, take it in the row, fetch it with `$.use(Trait)` |
| compute from values you already have | no row: take the values as parameters |
| take a callback that may print or read a file | a row parameter: `fn[$ R](...) $ R` |
| wire real objects, or a fake in a test | `$.with(Trait=box(...))` in `main`, or in the test ([testing](testing.md)) |

## Example: a service with its own effects

`check_all` pages the on-call team about full disks. It needs three
things from outside: `Pager`, `Disks` and `Clock`. The real providers go
in `main`. The test installs a scripted disk and a manual clock.

```hd
use std.time.{Clock, ManualClock, Timestamp, now}

pub trait Pager:
    fn page(&mut self, team: string, text: string) -> void

pub trait Disks:
    fn used_percent(&self, mount: string) -> u32

fn check_mount(mount: string, limit: u32) -> void $ Pager + Disks + Clock:
    used := $.use(Disks).used_percent(mount)
    if used >= limit:
        $.use(Pager).page("infra", "${now()} $mount is $used% full")

pub fn check_all(mounts: List[string]) -> void $ Pager + Disks + Clock:
    for mount in mounts:
        check_mount(mount, 90)

struct DigestPager:
    lines: List[string]

impl Pager for DigestPager:
    fn page(&mut self, team: string, text: string) -> void:
        self.lines.push("[$team] $text")

struct StaticDisks:
    percent: u32

impl Disks for StaticDisks:
    fn used_percent(&self, mount: string) -> u32:
        self.percent

pub fn main() -> void $ Console + Clock:
    digest := box(DigestPager { lines: [] })
    $.with(Pager=digest, Disks=box(StaticDisks { percent: 95 })):
        check_all(["/", "/data"])
    println("${digest.lines.len()} pages")

tests:
    struct ScriptedDisks:
        full: List[string]

    impl Disks for ScriptedDisks:
        fn used_percent(&self, mount: string) -> u32:
            if self.full.contains(&mount): 99 else: 10

    it("pages for a full disk only"):
        pager := box(DigestPager { lines: [] })
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Pager=pager, Disks=box(ScriptedDisks { full: ["/data"] }), Clock=clock):
            check_all(["/", "/data"])
        assert_equal(pager.lines, ["[infra] 1970-01-01T00:00:00Z /data is 99% full"])
```

```output
2 pages
```

- `check_all` says what it reaches. A caller reads that line, not the body.
- `main` installs the real providers. Its own row lists only host
  capabilities.
- The test keeps `pager`, so it can read what the code sent.

## Rows

A function's row must cover the row of everything it calls. Rows join with
`+`. Leaving one out is `missing-requirement`, naming what to add.

```text
error: p.hd:13:5: missing-requirement: this needs `$ Pager`, which the enclosing function's row does not name
```

| Function | Its row |
| --- | --- |
| `pub fn`, a `pub` method, a trait method | written; no `$` means the empty row |
| private `fn`, private method, local `fn`, closure | written, or inferred from its body when omitted |
| a script (top-level statements, no `main`) | every host capability, with no row written |

A `$.with` block inside a body supplies what it wraps, so the function's
row leaves that requirement out:

```text
pub fn local_wiring() -> u32:      # no row
    $.with(Disks=box(StaticDisks { percent: 7 })):
        $.use(Disks).used_percent("/")
```

A row alias names a long row: `type Ops = $ Pager + Disks + Clock`, used
as `$ Ops + Console`. It works only inside a row, never as `$.use(Ops)`.

## The host capabilities

These traits are built in. `hd` provides them to `main` when its row names
them. A package must also grant each one
([capabilities](capabilities.md)).

| Trait | Import from | Gives |
| --- | --- | --- |
| `Console` | prelude | `println`, `print`, and `eprintln` (import it from `std.console`) |
| `ConsoleInput` | `std.console` | `read_line!`, `read_all!` |
| `Args` | `std.cli` | the program's arguments |
| `Env`, `Sys` | `std.host` | environment variables, system facts |
| `Signals` | `std.host` | `$.use(Signals).wait!(.Interrupt)` for Ctrl-C |
| `Clock` | `std.time` | `now`, `sleep!` |
| `Random` | `std.random` | random numbers |
| `FsRead`, `FsWrite` | `std.fs` | reading and writing files |
| `Process` | `std.process` | running other programs |
| `Http`, `Net` | `std.http`, `std.net` | requests and sockets |
| `Workers` | `std.remote` | running code on a worker |

The std fake for each is in [testing](testing.md#standard-fakes).

## Your own effect

Any trait can be a requirement.

- **`$.use(Trait)`** fetches the current provider. Call its methods like
  any value's.
- **A provider is a box.** `$.with(Mailer=Outbox { sent: [] })` with a
  plain value is `type-mismatch`.
- **Box where you declare it.** `box(fake)` on an existing variable copies
  it, so the test then reads the untouched original.

A method that records calls changes itself, so the trait method takes
`&mut self`. Several providers go in one `$.with(A=a, B=b):`.

A `pub` function's row may name only `pub` traits. A private trait in a
`pub` row is `private-type-leak`.

### A provider that needs host capabilities

A trait method has the row the trait declares. An implementation must
match it exactly, or the compiler reports `trait-method-signature`. If a
provider prints, declare `$ Console` on the trait method. Every function
that calls the method then needs `Console` in its row, and so does every
fake.

```text
pub trait Pager:
    fn page(&mut self, text: string) -> void $ Console
```

Prefer a provider that needs nothing. Let it collect, and let `main`
print, as `DigestPager` does above.

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

`retry` has the row of whatever `body` has: pure here, `$ Clock` for a
body that reads the clock. A closure with no written row gets the row of
its body.

## With `!`

An effect method may suspend. Mark it `name!` in the trait, and call it
with `!`. Providers installed by `$.with` reach the tasks that `join!`,
`race!` and the other combinators start. The suspension rules are in
[concurrency](concurrency.md).

```hd
use std.task.join
use std.time.{Clock, ManualClock, Timestamp, ms, sleep}

pub trait Quotes:
    fn price!(&self, symbol: string) -> i32

struct Fixed: pass

impl Quotes for Fixed:
    fn price!(&self, symbol: string) -> i32:
        i32(symbol.len())

fn total!(symbols: List[string]) -> i32 $ Quotes + Clock:
    sleep!(5ms)
    quotes := $.use(Quotes)
    quotes.price!(symbols[0]) + quotes.price!(symbols[1])

fn both!() -> i32 $ Quotes + Clock:
    let (x, y) = join!(total(["ab", "c"]), total(["abc", "d"]))
    x + y

tests:
    it("gives each joined task the provider"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Quotes=box(Fixed {}), Clock=clock):
            assert_equal(both!(), 7)
```

## What should be an effect

| Make it an effect | Keep it a parameter or plain code |
| --- | --- |
| it reaches outside the program: network, disk, mail, a payment API | it is a calculation on values you hold |
| a test must replace it | the value differs on every call |
| it is needed deep in a call chain and would clutter every signature | one function uses it, and its caller already has it |
| time, randomness, anything nondeterministic | a constant or a setting read once in `main` |

- Put the trait around the outside edge, not around your own logic. A
  `Mailer` is an effect. A `TaxCalculator` is a function.
- Keep traits small, one job each, so a fake is a few lines.
- Configuration is data. Read it in `main`, then pass the struct down.

Where state is shared between holders rather than reached from anywhere,
see [shared state](../scenarios/shared-state.md#state-reached-from-anywhere-a-provider).
