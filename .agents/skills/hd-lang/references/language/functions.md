# Functions And Control Flow

## Functions

`fn name(param: T) -> R:` then an indented body, or a same-line body after
the colon. Parameter and result types are always written. The last
expression is the value; `return` leaves early. A function that returns
nothing returns `void`.

```hd
fn discounted(cents: i32, percent: i32 = 10) -> i32:    # a default value
    cents - cents * percent / 100

fn label(cents: i32) -> string:
    if cents == 0:
        return "free"
    "$cents cents"

fn sum(values...: List[i32]) -> i32:                    # a vararg collects the rest
    let total = +0
    for value in values:
        total += value
    total

println("${discounted(1000)} ${discounted(1000, percent=25)} ${label(0)} ${sum(1, 2, 3)}")
```

```output
900 750 free 6
```

A call may name an argument: `discounted(1000, percent=25)`. A parameter
is not reassignable, and neither is anything it holds: copy it into a
`let` or `let mut` first, or take `&mut` to change the caller's value
([references.md](references.md)).

## Generics

Type parameters go in `[...]`, a bound is written with `<`, and several
bounds join with `&`. `::[T]` gives type arguments at a call when nothing
else decides them.

```hd
fn first_or[T](items: List[T], fallback: T) -> T:
    match items.get(0):
        .Some(item) => item
        .None => fallback

fn show_all[T < Display](items: List[T]) -> string:
    items.map(fn(item): item.to_string()).join(", ")

println("${first_or::[string]([], "none")} ${first_or([+4], 0)} ${show_all([1.5, 2.5])}")
```

```output
none 4 1.5, 2.5
```

## Closures and function values

A closure is an unnamed `fn`. It reads and changes nearby variables. Its
parameter and result types may be left out where the call expects a
function type. A function's name is a value too.

```hd
fn apply_twice(f: fn(i32) -> i32, x: i32) -> i32: f(f(x))

fn double(x: i32) -> i32: x * 2

struct Handler:
    callback: fn(i32) -> i32

inc := fn(x: i32) -> i32: x + 1
let calls = 0
counted := fn(x: i32) -> i32:
    calls += 1
    x * 10
handler := Handler { callback: double }
println("${apply_twice(inc, 1)} ${apply_twice(double, 1)} ${apply_twice(fn(x): x - 1, 1)}")
println("${counted(2)} $calls ${(handler.callback)(4)}")       # call a field: parentheses
```

```output
3 4 -1
20 1 8
```

A function parameter whose callback prints or needs another capability
writes that row in the function type, as `visit: fn(i32) -> void $ Console`
([requirements.md](requirements.md)). A closure kept in a field or
returned needs more care ([references.md](references.md)).

When the last parameter is a zero-argument function, a call may give it as
an indented block:

```hd
fn twice(body: fn() -> void $ Console) -> void $ Console:
    body()
    body()

pub fn main() -> void $ Console:
    twice:
        println("again")
```

```output
again
again
```

## Pipes

`|>` feeds a value into the next step, and `_` marks where it goes. A step
with no `_` must be a function of one argument.

```hd
fn charge(grams: i32, rate: i32) -> i32: grams / 100 * rate

fn add_fee(cents: i32) -> i32: cents + 99

println(1200 |> charge(_, 40) |> add_fee)
```

```output
579
```

## Conditions and loops as expressions

`if`, `match`, and a loop with `else` produce values. Every branch must
give the same type.

```hd
fn grade(score: i32) -> string:
    match score:
        90..=100 => "excellent"
        n if n >= 50 => "passing"
        _ => "failing"

score := +72
label := if score >= 90: "top" else if score >= 50: "ok" else: "low"
found := for word in ["tea", "cake"]:
    if word.starts_with("c"):
        break word                      # the loop's value
else:
    "nothing"                           # the value when the loop ends without `break`
if (size := found.len()) > 3:           # `:=` inside an expression needs parentheses
    println("$found has $size bytes")
println("$label ${grade(score)}")
```

```output
cake has 4 bytes
ok passing
```

The control words are `if`, `else`, `match`, `for`, `while`, `break`,
`continue` and `return`. There is no `loop`, `elif`, `switch`, `do`, or
ternary `?:`.

## Cleanup with `defer`

`defer(action)` from `std.ops` registers a callback that runs when the
enclosing block exits, however it exits.

```hd
use std.ops.defer

fn audited(log: &mut List[string]) -> i32:
    defer(fn() -> void: log.push("closed"))
    log.push("opened")
    +42

let mut log: List[string] = []
value := audited(&mut log)
println("$value ${log.join(",")}")
```

```output
42 opened,closed
```
