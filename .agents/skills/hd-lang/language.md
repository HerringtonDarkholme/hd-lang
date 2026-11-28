# The hd Language

The core of hd on one page, then a short introduction to each larger topic.
Read it before writing any hd: code guessed from Python or Rust does not check.

## File shape

A file with top-level statements is a **script**: they run in order and may
use every host capability. A file with `pub fn main` is a **program**, and
`main` lists what it uses after `$` (`println` needs `$ Console`). Use
`main` in a package and in a file with `tests:`, since a test run runs no
script body. `#` starts a comment.

```hd
pub fn main() -> void $ Console:
    ok := (1 < 2
        && 2 < 3)                               # a continued expression needs parentheses
    shout := " hi "
        .trim()                                 # unless the next line starts with `.` or `|>`
        .upper()
    println("$ok $shout")                       # true HI
```

A block is a header ending in `:` plus an indented body (four spaces), or a
same-line body; `pass` is the empty body. There are no braces, no `end`, no
semicolons. A same-line body cannot hold `if` or `match` unless parenthesized.

## Bindings

`name := expr` is never reassigned and takes no type annotation.
`let name = expr` can be reassigned, and an annotation (`let name: T = ...`)
always needs `let`. `let mut` is for a list, map or struct changed in place
(`xs.push(v)`, `p.x = 1`). Destructuring is always `let (a, b) = pair`.

```hd
LIMIT := 3                      # a top-level constant, private to the file
let count = 0
count = count + LIMIT           # reassigning needs `let`
let mut seen: List[string] = ["a"]
seen.push("b")                  # changing in place needs `let mut`
let (low, high) = (1, 9)
_ := seen.pop()                 # `pop` gives an optional, which must be used
println(count + seen.len() + low + high)    # 14
```

An unused `Result` or optional is `discarded-must-use-value`: discard it
with `_ := expr`. A number or string needs only `let` (`hd lint` reports
`redundant-let-mut`). A public binding writes its type, as in
`pub let LIMIT: usize = 3`; importing modules may read it but never write it.

## Numbers and conversion

Types: `i8` `i16` `i32` `i64`, `u8` `u16` `u32` `u64`, `usize` (lengths,
indices, byte offsets), `f32`, `f64`. There is no `isize` and no `int`.

- An integer literal takes its type from how the body uses it. With no use
  that decides, it is `usize`, or `i32` when written with a sign, as `+5`.
  A float literal is `f64`. A suffix fixes the type (`1u32`, `-3i64`,
  `1.5f32`); a hex literal takes none (`let mask: u64 = 0xFF`).
- Conversions are calls: `i32(n)`, `usize(n)`, `f64(n)`. Nothing converts
  implicitly, and signed and unsigned integers do not mix. A narrowing call
  keeps the low bits; `u8::try_from(n)` gives a `Result` instead.
- `/` on integers truncates, `%` is the remainder, `**` a power. Overflow
  panics (with `--release` it wraps), as does division by zero.

```hd
use std.convert.TryFrom
use std.num.Integer             # for `max_value` and `min_value`

scores := [+90, 85]
last := i32(scores.len()) - 1           # `len()` is usize; convert before signed arithmetic
mean := f64(scores[0] + scores[1]) / f64(scores.len())
wide := 300                             # `u8(300)` is `integer-literal-range`
byte := u8::try_from(+300).ok()         # `.None`: outside 0..=255
# prints 87.5 1 3 1 1024 255 44 Option.None 2147483647
println("$mean $last ${7 / 2} ${7 % 2} ${2 ** 10} ${0xFF} ${u8(wide)} ${debug(byte)} ${i32::max_value()}")
```

## Strings and interpolation

A `string` is immutable UTF-8 text. `"$name"` and `"${expression}"`
interpolate; `"\$"` is a dollar sign and `"{x}"` prints the braces. A `char`
is `'a'`. `len()`, `slice` and `find` count bytes; `s.chars().count()`
counts characters.

```hd
use std.text.r
name := "Ada"
line := "$name has ${1 + 2} items, first letter ${'A'}, cost \$6"
pattern := r"\d+ items"         # a raw string: backslashes stay as written
println("$line / $pattern / ${line.len()} bytes / {name}")
```

```output
Ada has 3 items, first letter A, cost $6 / \d+ items / 40 bytes / {name}
```

A multiline string is `"""..."""`. It interpolates and takes escapes like
`"..."`, and keeps its lines exactly as written: no dedent, no trim. Use it
for text with several lines, such as generated source, a report or a
template, rather than joining `"\n"` pieces or pushing line by line.

```hd
use std.console.print
name := "Record"
field := "count"
source := """struct ${name}:
    ${field}: i64

fn make() -> ${name}:
    ${name} { ${field}: 1 }
"""
print(source)
```

```output
struct Record:
    count: i64

fn make() -> Record:
    Record { count: 1 }
```

Only a `Display` type interpolates: numbers, `bool`, `char`, `string`, and
types that implement `Display`. For an optional, list, map or struct write
`${debug(x)}`. `+` joins two strings; in a loop use `parts.join(",")` or a
`StringBuilder` ([std.md](std.md)). `dbg(x)` prints a value and its location
to standard error and returns `void`: a statement, not a wrapper.

## Control flow

`if`, `match`, and a loop with `else` are expressions; every branch gives
the same type. The operators are `!`, `&&` and `||`.

```hd
score := +72
label := if score >= 90: "top" else if score >= 50: "ok" else: "low"
grade := match score:
    90..=100 => "excellent"
    n if n >= 50 => "passing"
    _ => "failing"
found := for word in ["tea", "cake"]:
    if word.starts_with("c"):
        break word                      # the loop's value
else:
    "nothing"                           # the value when no `break` ran
if (size := found.len()) > 3 && !found.is_empty():   # `:=` in an expression needs parentheses
    println("$label $grade $found $size")            # ok passing cake 4
```

The control words are `if`, `else`, `match`, `for`, `while`, `break`,
`continue` and `return`. Write `else if` and `while true:`. There is no
`not`, `and`, `or`, `elif`, `loop`, `switch`, `do`, or ternary `?:`.

## Functions

Parameter and result types are always written. The last expression is the
value, `return` leaves early, and a function with no result returns `void`.

```hd
use std.ops.defer

fn discounted(cents: i32, percent: i32 = 10) -> i32: cents - cents * percent / 100   # a default
fn sum(values...: List[i32]) -> i32: values.sum()       # a vararg collects the rest
fn apply_twice(f: fn(i32) -> i32, x: i32) -> i32: f(f(x))
fn add_fee(cents: i32) -> i32: cents + 99

fn audited(log: &mut List[string]) -> i32:
    defer(fn() -> void: log.push("closed"))     # runs when the block exits, however it exits
    log.push("opened")
    +42

fn twice(body: fn() -> void $ Console) -> void $ Console:
    body()
    body()

pub fn main() -> void $ Console:
    let calls = 0
    counted := fn(x: i32) -> i32:               # a closure may change nearby variables
        calls += 1
        x * 10
    let mut log: List[string] = []
    # prints 750 3 100 2 42 opened,closed
    println("${discounted(1000, percent=25)} ${sum(1, 2)} ${apply_twice(counted, 1)} $calls ${audited(&mut log)} ${log.join(",")}")
    println(1200 |> discounted(_, percent=50) |> add_fee |> apply_twice(fn(x): x - 1, _))   # 697
    twice:                                      # a last `fn()` parameter as a block
        println("again")                        # printed twice
```

- A call may name an argument, as `percent=25`. A parameter is not
  reassignable, and neither is anything it holds: copy it into a `let`, or
  take `&mut` to change the caller's value.
- A closure is an unnamed `fn`; its types may be left out where the call
  expects a function type. A function's name is a value too. Call a field
  holding one with parentheses: `(handler.callback)(4)`.
- `|>` feeds a value into the next step, and `_` marks where it goes; a
  step with no `_` must take one argument.
- A callback that prints has the row in its type: `fn(i32) -> void $ Console`
  ([design/effects.md](design/effects.md)). A closure kept in a field:
  [reference/references.md](reference/references.md).

## Collections

`List[T]` and `Map[K, V]` need no import. Both are values: `b := a` copies.
A key type needs `Eq` and `Hash`: strings, numbers and `@derive(Eq, Hash)`
types. `{}` is the empty map. Methods are in [std.md](std.md).

```hd
let mut names: List[string] = []        # an empty literal needs a type
names.push("Grace")
maybe := names.get(9)                   # `.None` when out of range; `names[9]` panics
more := [names..., "Ada"]               # spread into a new list
first := more.sorted().slice(0, 1)      # new lists; `sort()` sorts in place; no `xs[a..b]`
let mut stock: Map[string, i32] = {"tea": 3, "jam": 12}
stock["cake"] = 1                       # insert or replace
stock["tea"] += 2                       # the key must exist, or it panics
pair := (stock.get("tea").unwrap_or(0), "one")    # read with `pair._0`, or `let (a, b) = pair`
let total = +0
for (name, count) in stock:             # a map loop lends `(&K, &V)`
    total += count                      # operators read through a reference
    if *count > 10:                     # `*count` copies the value out
        println("many $name")
let mut prices = [+100, 250]
for price in &mut prices:               # change each element in place
    *price += 1
evens := [for i in 0..6 if i % 2 == 0 => i * 10]    # `0..6` stops before 6; `0..=6` includes it
lengths := {for name in more => name: name.len()}
let fees: List[i32] = prices.iter().filter(fn(p): p >= 200).map(fn(p): p / 10).collect()
# prints `many jam`, then: Option.None Ada 5 one 18 [101, 251] [0, 20, 40] 3 [25]
println("${debug(maybe)} ${first[0]} ${pair._0} ${pair._1} $total ${debug(prices)} ${debug(evens)} ${lengths["Ada"]} ${debug(fees)}")
```

- A list loop gives copies, and walks the list as it was when it began.
  Pass `*count` where a function wants a map value itself. A comprehension
  puts clauses first, then `=>` and the value; `[for a in xs for b in ys => a * b]`.
- `iter()` starts a lazy chain: `map` and `filter` describe work, and
  `collect`, `sum`, `count` or a `for` loop runs it. A `filter` callback
  takes a reference, a `map` callback the value. Eager list helpers skip
  `iter()`: `xs.map(f)`, `xs.filter(f)`, `xs.any(f)`, `xs.find(f)`.
- `collect`, `sum` and `product` need a result type from a `let` annotation,
  the return type, or `::[T]` (`xs.iter().sum::[i32]()`), else `cannot-infer-type`.

## Data: structs, enums, match

A `struct` has named, typed fields and is a value. Methods live in an
`impl` block. An `enum` is a closed set of variants that may carry named
fields, and a `match` must cover every case.

```hd
type UserId(string)                     # a newtype; `type Scores = Map[string, i32]` is an alias

struct Item:
    name: string
    cents: i32
    tags: List[string] = []             # a default; a literal may leave it out

impl Item:
    fn free(name: string) -> Item: Item { name, cents: 0 }  # `name` is `name: name`
    fn raise(&mut self, by: i32) -> void:
        self.cents += by

@derive(Eq, Debug)
enum Status:
    Pending
    Refunded(cents: i32, reason: string)

fn describe(status: Status) -> string:
    match status:
        .Pending => "pending"
        .Refunded(cents, _) if cents > 10000 => "large refund"
        .Refunded(cents, reason) =>
            note := "refunded $cents"
            "$note: $reason"

fn first_tag(item: Item) -> string:
    let .Some(tag) = item.tags.get(0) else: return "untagged"
    tag

gift := Item { ...Item::free("tea"), tags: ["gift"] }    # copy, with changes
let mut cake = Item::free("cake")
cake.raise(400)                         # `&mut self` needs a `let mut` variable
status := Status.Refunded(250, "late")  # the full name, where the type is not known
println("${first_tag(gift)} ${first_tag(cake)} ${cake.cents} ${string(UserId("u-1"))}")   # gift untagged 400 u-1
# prints refunded 250: late pending false Status.Refunded(cents=250, reason="late")
println("${describe(status)} ${describe(.Pending)} ${status == .Pending} ${debug(status)}")
```

- Build a struct with `Name { field: value }`, never `Name(field: value)`.
  The spread is `...base`, with three dots, and comes first.
  `struct Empty: pass` has no fields; build it with `Empty {}`.
- `&self` reads in place, `&mut self` changes the value (on a `let mut`
  variable or through a `&mut`), and `self` gets a read-only copy. With no
  receiver the function is associated: `Item::free("cake")`.
  A generic type names its parameters twice: `struct Stack[T]:`, `impl[T] Stack[T]:`.
- `.Variant` works where the type is known (an argument, a return value, a
  comparison, an arm); elsewhere write `Status.Variant`.
- An arm is `pattern => expression` or an indented body, never `{ ... }`.
  One pattern per arm: there is no `A | B`; write an arm for each, or `_`.
  Patterns: a literal, a range (`1..=9`), a variant with payload names, a
  tuple (`(0, name)`), `_`, and a name. A guard is `pattern if condition`.
- `let PATTERN = value else: ...` binds on a match; the `else` block must
  leave with `return`, `break`, `continue` or `panic`.
- A newtype is unwrapped by the base type's name: `string(id)`.

## Modules

Each file under a package's `src/` is a module named by its path:
`src/pricing.hd` is `pkg.pricing`, and a folder's `mod.hd` is the folder
(`src/cart/mod.hd` is `pkg.cart`). `src/main.hd` holds `main`; `src/lib.hd`
is a library's root ([reference/packages.md](reference/packages.md)). A
declaration is private to its module unless marked `pub`.

```hd
use std.cmp.max                 # `use` goes at the top, never inside a function

## The price of one line of a cart.
pub struct Line:
    pub item: string            # a field other modules read is `pub` too
    pub cents: i32

pub fn total(lines: List[Line]) -> i32: max(0, lines.map(fn(l): l.cents).sum() + fee())

fn fee() -> i32: 99             # private: only this module sees it
```

- `use` may also open a `tests:` block. Forms: `use std.fs.{FsRead, read_text}`,
  `use pkg.pricing` (then `pricing.total(...)`), `use pkg.pricing.{total as sum_of}`.
  `pub use self.format.{receipt}` re-exports, always with braces. A path starts
  at `std`, `pkg` (this package's `src/`), `dep.NAME`, `self` or `super`; a
  single file outside a package may import only `std`.
- A field is private unless `pub`, even in a `pub struct`. Another module,
  including a test under `tests/`, that reads it or builds the struct gets
  `private-member`: mark each field `pub`, or export a constructor.
- A `pub fn` may name only `pub` types and traits in its signature and row
  (`private-type-leak`). Importing a private name is `private-import`. A
  name may not shadow a prelude name such as `List` (`prelude-name-shadow`).
- Folders may not import each other in a circle (`folder-cycle`): move a
  module both need into a folder of its own. `##` is a doc comment on the
  declaration right below it, with no blank line between. `hd doc` reads it,
  and a fenced `hd` block in one is a doc test.

## Errors

There is no `null` and no exceptions. `T?` is `.Some(value)` or `.None`;
`Result[T, E]` is `.Ok(value)` or `.Err(error)`. Postfix `?` unwraps, or
returns the `.None` or `.Err` from a function whose return type matches
(else `invalid-result-propagation`). `@error` writes an enum's messages.
`unwrap()` panics: prefer `match`, `?`, `unwrap_or(x)` or `expect("why")`
([design/errors.md](design/errors.md)).

```hd
@error
enum CouponError:
    @error("unknown coupon $code")
    Unknown(code: string)
fn percent(code: string) -> Result[i32, CouponError]:
    digits := code.strip_prefix("SAVE").ok_or(CouponError.Unknown(code))?   # `.Err` returns here
    .Ok(digits.parse::[i32]().unwrap_or(0))

match percent("x"):
    .Ok(p) => println("$p% off")
    .Err(error) => println("error: $error")     # error: unknown coupon x
```

## Traits and generics

A trait lists methods; each implementation is an explicit
`impl Trait for Type`. Type parameters go in `[...]`, a bound is written
with `<`, and bounds join with `&`. `::[T]` gives type arguments at a call
when nothing else decides them. A trait used as a type is `&dyn Trait` or
`*dyn Trait`. More in [reference/traits.md](reference/traits.md).

```hd
trait Shape:
    fn area(&self) -> f64
    fn name(&self) -> string: "shape"   # a default body
@derive(Debug)
struct Square:
    side: f64
impl Shape for Square:
    fn area(&self) -> f64: self.side * self.side

fn total[T < Shape & Debug](shapes: List[T]) -> f64: shapes.map(fn(s): s.area()).sum()

println("${total([Square { side: 2.0 }])} ${total::[Square]([])} ${Square { side: 1.0 }.name()}")  # 4.0 0.0 shape
```

## Values and references

A struct, enum, tuple, list, map and string are values: assigning, passing
or returning one copies it. `&x` lends a variable to one call to read, and
`&mut x` to change; the variable is `let mut`. `box(value)` makes a shared
object (`*mut T`) for fields, collections and return values
([reference/references.md](reference/references.md)).

```hd
fn restock(units: &mut List[i32], added: i32) -> void:
    units.push(added)

let mut local = [+2]
saved := local                          # a copy
restock(&mut local, 3)                  # the call site writes `&mut` too
shared := box([1])
alias := shared                         # the same object
alias.push(9)
println("${saved.len()} ${local.len()} ${shared.len()} ${shared is alias}")   # 1 2 2 true
```

## Requirements: `$`

Everything a function needs from outside is in its row after `$`, joined
with `+`: `$ Console + Clock`. A function that prints needs `$ Console`, and
so does every function that calls it (`missing-requirement`). A function
with no `$` is pure. Any trait can be a requirement: `$.use(Trait)` fetches
its provider and `$.with(Trait=box(value)):` installs one, so a test swaps
in a fake ([design/effects.md](design/effects.md);
[design/capabilities.md](design/capabilities.md) grants them in `hd.toml`).

```hd
use std.time.{Clock, now}
fn stamp(message: string) -> string $ Clock: "${now().unix_milliseconds() > 0}: $message"

pub fn main() -> void $ Console + Clock:
    println(stamp("hello"))
```

## Waiting: `!`

A function that can wait (input, files, timers, the network) has a name
ending in `!`, and every call to it is written with `!`. Only a `!`
function may make a bang call (`bang-call-outside-suspension`), so the mark
travels up to `main!`. Import one by its plain name. `join!` and `race!`
wait for several calls at once ([design/concurrency.md](design/concurrency.md)).

```hd
use std.time.{Clock, ms, sleep}
pub fn main!() -> void $ Clock + Console:
    sleep!(1ms)
    println("ready")
```

## Tests

One `tests:` block at the end of a file holds its unit tests, run with
`hd test`. Each `it("name"):` is a case. Test code may call private
functions; `it`, `assert` and `assert_equal` need no import
([design/testing.md](design/testing.md)).

```hd
fn shipping(grams: i32) -> i32: (if grams <= 1000: 499 else: 499 + (grams - 1000) / 500 * 150)

tests:
    it("each full 500 g adds 150"):
        assert_equal(shipping(2000), 799, reason="two steps over 1 kg")
```

## Derives

`@derive(...)` writes a trait's implementation from a type's fields:
equality, ordering, hashing, debug text, JSON, command-line options,
validation and test data. Values copy, so there is no `Clone`
([design/derive.md](design/derive.md)).

```hd
@derive(Eq, PartialOrd, Ord, Hash, Debug)
struct Version:
    major: u32
    minor: u32

println("${Version { major: 1, minor: 2 } < Version { major: 1, minor: 10 }}")   # true
```

## Habits from other languages

| Habit (Python / Rust / Go / TypeScript) | hd |
| --- | --- |
| `elif`, `not` / `and` / `or`, `c ? a : b`, `loop` | `else if`, `!` / `&&` / `\|\|`, `if c: a else: b`, `while true:` |
| `[f(x) for x in xs if c]`, `lambda x: x + 1` | `[for x in xs if c => f(x)]`, `fn(x): x + 1` |
| a class, `__init__`, inheritance | `struct` + `impl`, and a `trait` for shared behavior |
| `None`, `null`, `x as i32`, `int(text)` | `T?` (`.Some(x)`, `.None`), `i32(x)`, `text.parse::[i32]()` |
| `raise`, `throw`, `if err != nil` | `return .Err(...)`; `call()?` passes it up |
| `Rc<RefCell<T>>`; `async` / `await` | `box(value)`; a `!` function: `fn load!()`, called `load!()` |
| a global, a DI container | a trait in the row, `$.use(Trait)`, `$.with(Trait=...)` |
| `if __name__ == "__main__"`, `func main()` | `pub fn main() -> void $ Console:` |

The full guide, with errors, text, programs and a side-by-side port, is
[scenarios/porting.md](scenarios/porting.md).
