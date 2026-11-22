# Data: Structs, Enums, Match

## Structs

A `struct` is a record with named, typed fields. It is a value: assigning
or passing one copies it.

```hd
struct Item:
    name: string
    cents: i32
    tags: List[string] = []             # a default; a literal may leave it out

tea := Item { name: "tea", cents: 250 }
gift := Item { ...tea, name: "gift tea", tags: ["gift"] }   # copy `tea`, with changes
let mut cart = Item { name: "cake", cents: 400 }
cart.cents += 50                        # changing a field needs `let mut`
println("${tea.name} ${gift.name} ${gift.tags.len()} ${cart.cents}")
```

```output
tea gift tea 1 450
```

- Build one with `Name { field: value }`, never `Name(field: value)`.
- The spread is `...base` and comes first.
- `struct Empty: pass` declares a struct with no fields; build it with
  `Empty {}`.
- In a `pub struct`, each field another module reads is `pub` too.

## Methods

Methods live in an `impl` block. The receiver says what the method does:

| Receiver | Meaning | Called on |
| --- | --- | --- |
| `&self` | reads the value in place | anything |
| `&mut self` | changes the value | a `let mut` variable, or through a `&mut` |
| `self` | gets a read-only copy | anything |
| none | an associated function | the type: `Point::origin()` |

```hd
struct Point:
    x: i32
    y: i32

impl Point:
    fn origin() -> Point: Point { x: 0, y: 0 }
    fn len2(&self) -> i32: self.x * self.x + self.y * self.y
    fn shift(&mut self, dx: i32) -> void:
        self.x += dx

let mut p = Point::origin()
p.shift(3)
println("${p.x} ${p.len2()}")
```

```output
3 9
```

A generic struct takes its parameters on both the struct and the `impl`:

```hd
struct Stack[T]:
    items: List[T]

impl[T] Stack[T]:
    fn new() -> Stack[T]: Stack { items: [] }
    fn push(&mut self, item: T) -> void:
        self.items.push(item)
    fn top(&self) -> T?:
        if self.items.is_empty(): .None else: .Some(self.items[self.items.len() - 1])

let mut stack: Stack[i32] = Stack::new()
stack.push(4)
println(debug(stack.top()))
```

```output
Option.Some(4)
```

## Enums

An `enum` is a closed set of variants. A variant may carry named fields.

```hd
@derive(Eq, Debug)
enum Status:
    Pending
    Paid(cents: i32)
    Refunded(cents: i32, reason: string)

fn describe(status: Status) -> string:
    match status:
        .Pending => "pending"
        .Paid(cents) if cents > 10000 => "paid (large)"
        .Paid(cents) => "paid $cents"
        .Refunded(cents, reason) => "refunded $cents: $reason"

status := Status.Paid(250)              # the full name, where the type is not known
println("${describe(status)} ${describe(.Pending)} ${status == .Paid(250)} ${debug(status)}")
```

```output
paid 250 pending true Status.Paid(cents=250)
```

`.Variant` works wherever the type is already known: an argument, a return
value, a comparison, a `match` arm. Elsewhere write `Status.Variant`.

## Match

A `match` must cover every case. An arm is `pattern => expression`, or
`pattern =>` followed by an indented body. There are no braces, and one
pattern per arm: write a separate arm for each, or `_` for the rest.

```hd
enum Shape:
    Circle(radius: f64)
    Rect(width: f64, height: f64)

fn area(shape: Shape) -> f64:
    match shape:
        .Circle(radius) => 3.0 * radius * radius
        .Rect(width, height) =>
            scaled := width * height
            scaled

fn classify(pair: (i32, string)) -> string:
    match pair:
        (0, name) => "zero $name"
        (n, _) if n < 0 => "negative"
        (1..=9, name) => "small $name"
        _ => "big"

println("${area(.Rect(2.0, 3.0))} ${classify((0, "a"))} ${classify((5, "b"))} ${classify((50, "c"))}")
```

```output
6.0 zero a small b big
```

Patterns: a literal (`0`, `"go"`, `true`), a range (`1..=9`), a variant
with payload names (`.Paid(cents)`), a tuple, `_` for anything, and a name
that binds the value. A guard is `pattern if condition`.

## Let-else

`let PATTERN = value else: ...` binds when the pattern matches. The `else`
block must leave: `return`, `break`, `continue` or `panic`.

```hd
fn first_tag(tags: List[string]) -> string:
    let .Some(tag) = tags.get(0) else: return "untagged"
    tag.upper()

println("${first_tag(["gift"])} ${first_tag([])}")
```

```output
GIFT untagged
```

## Aliases and newtypes

`type Name = T` is another name for the same type. `type Name(T)` is a new,
distinct type that wraps one value; the base type's name unwraps it.

```hd
type Scores = Map[string, i32]
type UserId(string)

let scores: Scores = {"ada": 10}
id := UserId("u-1")
println("${scores.len()} ${string(id)}")
```

```output
1 u-1
```
