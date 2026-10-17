---
name: write-hd
description: Write hd code that checks on the first or second try - a compact syntax reference by example (entry points, bindings and +0, functions, data, enums and match, Result and ?, traits and dyn, operator traits, requirement rows and $.with, bang calls, modules, tests:), the prelude and where std names live, and a table of the mistakes agents make most with their fixes. Use before writing or editing any .hd file. Pair it with the hd-cli skill for the check loop.
---

# Write hd Code

hd is indentation-based and statically typed. A header ends in `:` and
opens an indented body or a same-line body; there are no braces around
blocks. This page is a working reference. Every `hd` block below passes
`hd check` as a whole file. A block fenced `text` is a fragment.

For the details, read the source instead of guessing:

- [Learn hd in 10 minutes](../../../website/guide/LEARN_IN_10_MINUTES.md):
  the syntax on one page.
- [The tour](../../../website/tour/01-hello.md): one checked lesson per topic.
- [The spec](../../../spec/README.md): `spec/lang/` for the language,
  `spec/std/` for each std module. It is the authority.
- `hd doc std.text.split` prints the signature and summary of any std
  item, and `hd doc std.text` prints a whole module. It runs inside a
  package (a folder with `hd.toml`); see the hd-cli skill.

## Entry Points

A single file can be a script of top-level statements:

```hd
name := "Ada"
println("hello, $name")
```

`hd FILE` runs it, and `hd check FILE` checks it the same way. A test
run runs no script body, so `hd test` and `hd check --tests` report
`missing-requirement` on a top-level `println`: a file with a `tests:`
block keeps its work in `main`.

A program's entry point is a public `main`. It names every capability it
uses in its requirement row after `$`; `println` needs `$ Console`.

```hd
pub fn main() -> void $ Console:
    println("hello")
```

A program that reads input or files makes bang calls (`read_line!()`), so
its entry point is `main!`. `?` needs a `Result` return type, so return
`Result[void, E]` and end with `.Ok(())`. Rows join with `+`.

```hd
use std.console.{ConsoleInput, read_line}
use std.host.{Args, args}

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput + Args:
    prefix := match args().get(0):
        .Some(word) => word
        .None => ">"
    let lines: usize = 0
    while true:
        line := match read_line!()?:
            .Some(text) => text
            .None => break
        lines = lines + 1
        println("$prefix ${line.trim()}")
    println("lines: $lines")
    .Ok(())
```

Files work the same way: `read_text!(Path("notes.txt"))` from `std.fs`
needs `$ FsRead` and returns `Result[string, FsError]`.

## Bindings And Numbers

- `name := expr` binds a name that is never reassigned, at top level or in
  a body. `let name = expr` declares one that can be reassigned. A type
  annotation needs `let`: `let total: i64 = 0`.
- Top-level bindings are private. There is no `pub let` and no
  `pub NAME :=`; export a function instead.
- An integer literal with no context is `usize`. Write `+0` for an `i32`.
  `len()` returns `usize` too, and `usize` does not mix with `i32`: cast
  with `i32(n)` or `usize(n)`. Casts are calls; nothing is implicit.
- A list or map you change is `mut`: `let xs: mut List[string] = []`.
  `mut` goes on the type, never `let mut x` for a number or string.
- Tuples: `pair._0`, `pair._1`, or `let (a, b) = pair`.

```hd
LIMIT := +3

fn square(x: i32) -> i32: x * x

pub fn main() -> void $ Console:
    let count = +0
    count = count + square(LIMIT)
    let names: mut List[string] = []
    names.push("Ada")
    pair := (+1, "one")
    let counts: mut Map[string, usize] = {}
    for word in "a b a".split_whitespace():
        counts[word] = counts.get(word).unwrap_or(0) + 1
    more := [names..., "Grace"]
    println("$count ${pair._1} ${more.len()} ${counts.get("a").unwrap_or(0)}")
```

Strings interpolate `$name` and `${expr}`; write `"\$"` for a dollar sign.
Only a `Display` type interpolates: unwrap an optional first
(`.unwrap_or("")`), and print a list or a record with `debug(x)`.
To build text in a loop, push to a `StringBuilder` (`use std.text.StringBuilder`;
`let out: mut StringBuilder = StringBuilder::new()`, `out.push(s)`,
`out.build()`) or collect parts and `join` them. `out = out + s` in a loop
copies the whole text every time, as Go's `s += t` does.

## Functions And Control Flow

`fn f(a: T) -> R:` then an indented body, or a same-line body after the
colon. The last expression is the value. Generics go in `[...]`, bounds
with `<`, and `::[T]` gives explicit type arguments at a call.
There is no `loop` (write `while true:`), no `elif` (write `else if`), and
no `not`/`and`/`or` (write `!`, `&&`, `||`).

```hd
use std.cmp.max

fn discounted(cents: i32, percent: i32) -> i32:
    cents - cents * percent / 100

fn first_or[T](items: List[T], fallback: T) -> T:
    match items.get(0):
        .Some(item) => item
        .None => fallback

fn audited(log: mut List[string]) -> i32:
    defer:
        log.push("closed")
    log.push("opened")
    +42

pub fn main() -> void $ Console:
    let log: mut List[string] = []
    value := audited(log)
    scores := {"ada": +10, "grace": +12}
    let best = +0
    for (_, score) in scores:
        best = max(best, score)
    label := if best >= 12: "high" else: "low"
    high := [for (name, score) in scores if score > 10 => name]
    inc := fn(x: i32) -> i32: x + 1
    price := +1000 |> discounted(_, 10) |> inc
    empty := first_or::[string]([], "none")
    found := for word in ["tea", "cake"]:
        if word.starts_with("c"):
            break word
    else:
        "nothing"
    println("$value ${log.join(",")} $label ${high.len()} $price $empty $found")
```

`defer:` takes a suite (`defer: save()` on one line, or an indented body).
It runs when the block exits, and it cannot print, make bang calls, or
use `?`. `dbg(x)` prints `x` with its location to stderr and returns `void`,
so it is a statement, not a wrapper.

## Data, Enums, Match, Errors

```hd
use std.num.parse_i32

data Item:
    name: string
    cents: i32
    tags: List[string] = []

@derive(Eq)
enum Status:
    Pending
    Paid(cents: i32)
    Refunded(cents: i32, reason: string)

@error
enum OrderError:
    @error("empty order")
    Empty
    @error("bad quantity: $text")
    BadQuantity(text: string)

fn describe(status: Status) -> string:
    match status:
        .Pending => "pending"
        .Paid(cents) if cents > 10000 => "paid (large)"
        .Paid(cents) => "paid $cents"
        .Refunded(cents, reason) => "refunded $cents: $reason"

fn quantity(text: string) -> Result[i32, OrderError]:
    match parse_i32(text):
        .Ok(n) => .Ok(n)
        .Err(_) => .Err(OrderError.BadQuantity(text))

fn order_total(items: List[Item], qty_text: string) -> Result[i32, OrderError]:
    if items.is_empty():
        return .Err(OrderError.Empty)
    qty := quantity(qty_text)?
    let sum = +0
    for item in items:
        sum = sum + item.cents
    .Ok(sum * qty)

pub fn main() -> void $ Console:
    tea := Item { name: "tea", cents: 250 }
    gift := Item { ...tea, name: "gift tea", tags: ["gift"] }
    println(describe(Status.Refunded(250, "damaged")))
    match order_total([tea, gift], "3"):
        .Ok(total) => println("total: $total")
        .Err(error) => println("error: $error")
    if Status.Pending == Status.Pending:
        println("same")
    let nickname: string? = .None
    println(nickname.unwrap_or("no nickname"))
```

- Build a record with `Item { field: value }`, never `Item(field: v)`.
  `...tea` copies the rest. Fields of a `pub data` that other modules read
  are `pub` too.
- `.Variant` works where the type is known; otherwise `Status.Paid(1)`.
- `match` must be exhaustive. An arm is `pattern => expr` or an indented
  body, never `{ ... }`.
- `T?` is `Option[T]` (`.Some(x)`/`.None`); `Result[T, E]` is
  `.Ok(x)`/`.Err(e)`. `?` returns the `.None` or `.Err` early.
- `==` on your own type needs `@derive(Eq)`; `debug(x)` needs
  `@derive(Debug)`. JSON needs `@derive(Serialize, Deserialize)` and
  `use std.serde.{Deserialize, Serialize}`; decode with
  `decode::[Config](text)` from `std.json`.

## Traits, dyn, And Operators

A bound `[T < Shape]` dispatches statically. A trait used as a value type
is `dyn Shape`; a bare `List[Shape]` is `trait-used-as-type`. Operator
traits live in `std.ops` and must be imported; the result type is
`type Out`.

```hd
use std.ops.Add

trait Shape:
    fn area(self) -> f64
    fn name(self) -> string: "shape"

data Circle:
    radius: f64

data Rect:
    width: f64
    height: f64

impl Shape for Circle:
    fn area(self) -> f64: 3.14159 * self.radius * self.radius
    fn name(self) -> string: "circle"

impl Shape for Rect:
    fn area(self) -> f64: self.width * self.height

fn largest[T < Shape](shapes: List[T]) -> f64:
    let best = 0.0
    for shape in shapes:
        if shape.area() > best:
            best = shape.area()
    best

fn total_area(shapes: List[dyn Shape]) -> f64:
    let sum = 0.0
    for shape in shapes:
        sum = sum + shape.area()
    sum

@derive(Eq, Debug)
data Money:
    cents: i64

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { cents: self.cents + rhs.cents }

pub fn main() -> void $ Console:
    let shapes: List[dyn Shape] = [Circle { radius: 1.0 }, Rect { width: 2.0, height: 3.0 }]
    println("total: ${total_area(shapes)}")
    println("largest: ${largest([Rect { width: 1.0, height: 1.0 }])}")
    println(debug(Money { cents: 250 } + Money { cents: 100 }))
```

A method that changes `self` takes `mut self`, and a caller needs a mutable
value: `let mut parser = Parser { ... }`.

## Requirements, Providers, And Tests

A row lists the traits a function needs. `$.use(T)` fetches the provider,
and `$.with(T=value):` supplies one for a block, so a test swaps in a fake.
A public function's row may name only public traits. Tests live in one
`tests:` block at the end of the file; each `it("name"):` is a case, and
assertions come from `std.testing` with a required `reason=`.

```hd
pub trait Mailer:
    fn send(mut self, to: string, body: string) -> void

data StdoutMailer: pass

impl Mailer for StdoutMailer:
    fn send(mut self, to: string, body: string) -> void:
        pass

pub fn remind(users: List[string]) -> usize $ Mailer:
    let mut mailer = $.use(Mailer)
    for user in users:
        mailer.send(user, "your invoice is due")
    users.len()

pub fn main() -> void $ Console:
    $.with(Mailer=StdoutMailer {}):
        count := remind(["ada@example.com"])
        println("sent $count")

tests:
    use std.testing.{assert, assert_equal}

    data FakeMailer:
        sent: mut List[string]

    impl Mailer for FakeMailer:
        fn send(mut self, to: string, body: string) -> void:
            self.sent.push(to)

    it("mails every user"):
        let mut fake = FakeMailer { sent: [] }
        $.with(Mailer=fake):
            assert_equal(remind(["a", "b"]), 2, reason="two users")
        assert_equal(fake.sent, ["a", "b"], reason="in order")
        assert(fake.sent.len() == 2, reason="two mails")
```

To test printing, provide `MemoryConsole` from `std.console` and read
`console.output()` ([tour: requirements](../../../website/tour/12-requirements.md)).
`it_each` and `it_prop` are in [tour: tests](../../../website/tour/13-tests.md).
A function that makes a bang call is itself `fn name!(...)`, and so is
every call to it ([tour: suspension](../../../website/tour/16-suspension.md)).

## Modules And Packages

Each file under `src/` is a module named by its path: `src/shop/cart.hd`
is `pkg.shop.cart`. Declarations are private unless `pub`. `use` goes at
the top of a file (or of a `tests:` block), never inside a function. Roots
are `pkg`, `std`, `dep`, `self` and `super`.

```text
# src/main.hd (fragment: needs src/shop/cart.hd and the money dependency)
use dep.money.greet
use pkg.shop.cart.{Line, total}

pub fn main() -> void $ Console:
    lines := [Line { name: "tea", cents: 250, quantity: 2 }]
    println(greet("shop"))
    println("total: ${total(lines)} cents")
```

An integration test in `tests/NAME.hd` has top-level `it(...)` cases and
no `tests:` block; `hd_run!("NAME")` runs the built program
([tour: packages](../../../website/tour/14-packages.md)). A doc test is an
`hd` fence inside a `##` doc comment, with its own `use` lines.

## Prelude And std

No `use` needed: the primitive types, `string`, `List`, `Map`, `Option`,
`Result`, `Display`, `Debug`, `Eq`, `Ord`, `Hash`, `Iterator`, `Console`,
`ConsoleError`, `println`, `panic`, `debug`, `dbg`, and `it` in tests
([full list](../../../spec/lang/10-modules.md#prelude)). Everything else
is imported:

| Need | Import |
| --- | --- |
| read input, stderr, test console | `std.console.{ConsoleInput, read_line, eprintln, MemoryConsole}` |
| program arguments, environment | `std.host.{Args, args, Env, env}` |
| files and paths | `std.fs.{FsRead, FsWrite, FsError, read_text, write_text}`, `std.path.Path` |
| parse numbers | `std.num.{parse_i32, parse_i64, parse_usize, parse_f64}` |
| `min`, `max`, `clamp`, `Ordering` helpers | `std.cmp` |
| `Set`, `Deque`, `Heap`, `counts` | `std.collections` |
| clock, durations, sleeping | `std.time.{Clock, now, sleep, ManualClock, ms, s}` |
| run tasks together | `std.task.{all, race}` |
| JSON | `std.json.{decode, encode, parse}`, `std.serde.{Serialize, Deserialize}` |
| operator traits | `std.ops.{Add, Sub, Mul, Index}` |
| errors | `std.error.{Error, report_of}`, `std.convert.From` |
| assertions, test helpers | `std.testing.{assert, assert_equal, it_each, it_prop, hd_run}` |

Import a bang function by its plain name (`read_text`) and call it with
the bang (`read_text!(...)`). String methods are in
[spec/std/text.md](../../../spec/std/text.md).

## Mistakes And Fixes

The mistakes agents make most often
([docs/audit/hd-writing-log.md](../../../docs/audit/hd-writing-log.md)), with
the code `hd check` reports today.

| Wrote | Code | Write instead |
| --- | --- | --- |
| `fn f() -> i32 {` | `syntax-error` | `fn f() -> i32:` and an indented body |
| `pub fn main() -> void:` calling `println` | `missing-requirement` | `pub fn main() -> void $ Console:` |
| `fn main()` with no `pub` | `private-main` warning, nothing runs | `pub fn main()` |
| `pub fn main()` calling `read_line!()` | `bang-call-outside-suspension` | `pub fn main!()` |
| `?` in `main!() -> void` | `invalid-result-propagation` | `-> Result[void, ConsoleError]`, end with `.Ok(())` |
| `$ Console, Clock` | `old-row-separator` | `$ Console + Clock` |
| `let i = 0` compared with an `i32` | `mixed-signedness` | `let i = +0`, or `let i: i32 = 0` |
| an `i32` counter (`+0`) plus `len()` or another `usize` | `mixed-signedness` (no hint) | keep counters `usize` (`0`), or convert explicitly: `i32(n)` |
| `s.len()` used as a character count | (none: wrong result) | `len()` counts bytes; characters are `s.chars().count()` |
| asserting a test got the error you expected | (none) | `assert_equal(r.is_err(), true, reason=...)`, or `match r:` with `.Err(.Low(balance=b)) => assert_equal(b, 5, ...)` and `.Ok(_) => panic("expected Low")` |
| `n := +0` then `n = n + 1` | `non-reassignable-binding` | `let n = +0` |
| `let mut n = +0` | `mut-on-primitive` | `let n = +0` |
| `total: i64 = 0` | `missing-let` | `let total: i64 = 0` |
| `pub let LIMIT = 3`, `pub LIMIT := 3` | `syntax-error` | private `LIMIT := +3`, plus `pub fn limit() -> i32: LIMIT` |
| `let xs: List[string] = []` then `xs.push(x)` | `mutable-receiver-required` | `let xs: mut List[string] = []` |
| `fn f(mut xs: List[i32])` | `syntax-error` | `fn f(xs: mut List[i32])` |
| `xs.append(x)` | `unknown-method` | `xs.push(x)` |
| `pair.0` | `syntax-error` | `pair._0` |
| `defer save()` | `syntax-error` | `defer: save()` |
| `println` inside `defer:` | `suspension-forbidden-context` | record into a list; print after |
| `List[Shape]` for a trait | `trait-used-as-type` | `List[dyn Shape]` (`hd fix` inserts it) |
| `impl Add for Money` with no import | `unknown-trait` | `use std.ops.Add` |
| `type Output = Money` | `extra-trait-member` | `type Out = Money` |
| `assert_equal(a, b)` | `argument-count` | `assert_equal(a, b, reason="...")` |
| `assert_equal` with no import | `unknown-name` | `use std.testing.assert_equal` in `tests:` |
| `if not ok`, `a and b`, `elif` | `syntax-error` | `!ok`, `a && b`, `else if` |
| `loop:` | `unknown-name` | `while true:` |
| `"{x}"` | prints the braces | `"${x}"` or `"$x"` |
| `"$opt"` with `opt: string?` | `unsatisfied-trait-bound` | `"${opt.unwrap_or("")}"` |
| `{:}` for an empty map | `syntax-error` | `let m: mut Map[K, V] = {}` |
| `use` inside a function | `syntax-error` | move it to the top of the file |
| `Item(name: "a")` | `trailing-block-position` | `Item { name: "a" }` |
| a second `tests:` block | `duplicate-tests-block` | one `tests:` block per file |
| a trait in a public row | `private-type-leak` | `pub trait` |
