# Traits And Generics

## Declaring and implementing

A trait lists methods. A method may have a default body. Each
implementation is an explicit `impl Trait for Type`.

```hd
trait Shape:
    fn area(&self) -> f64
    fn name(&self) -> string: "shape"       # a default body

struct Circle:
    radius: f64

struct Rect:
    width: f64
    height: f64

impl Shape for Circle:
    fn area(&self) -> f64: 3.0 * self.radius * self.radius
    fn name(&self) -> string: "circle"

impl Shape for Rect:
    fn area(&self) -> f64: self.width * self.height

println("${Circle { radius: 1.0 }.name()} ${Rect { width: 2.0, height: 3.0 }.name()}")
```

```output
circle shape
```

An implementation's method repeats the trait's signature exactly: the same
receiver, parameter types, result, `!`, and row after `$`.

## Static dispatch: bounds

`[T < Shape]` accepts any one type that implements the trait. Join several
bounds with `&`.

```hd
trait Shape:
    fn area(&self) -> f64

struct Rect:
    width: f64
    height: f64

impl Shape for Rect:
    fn area(&self) -> f64: self.width * self.height

fn largest[T < Shape](shapes: List[T]) -> f64:
    let best = 0.0
    for shape in shapes:
        if shape.area() > best:
            best = shape.area()
    best

fn show[T < Display & Eq](a: T, b: T) -> string:
    if a == b: "$a twice" else: "$a and $b"

println("${largest([Rect { width: 1.0, height: 2.0 }, Rect { width: 3.0, height: 1.0 }])} ${show(1, 2)}")
```

```output
3.0 1 and 2
```

## Dynamic dispatch: `dyn`

A trait used as a type is written with `dyn` and a reference, so one list
can hold different types. Use `&dyn Shape` for a parameter, and
`*dyn Shape` for a field or a collection element, filled with `box(...)`.

```hd
trait Shape:
    fn area(&self) -> f64

struct Circle:
    radius: f64

struct Rect:
    width: f64
    height: f64

impl Shape for Circle:
    fn area(&self) -> f64: 3.0 * self.radius * self.radius

impl Shape for Rect:
    fn area(&self) -> f64: self.width * self.height

fn describe(shape: &dyn Shape) -> string: "area ${shape.area()}"

fn total(shapes: List[*dyn Shape]) -> f64:
    let sum = 0.0
    for shape in shapes:
        sum += shape.area()
    sum

let shapes: List[*dyn Shape] = [box(Circle { radius: 1.0 }), box(Rect { width: 2.0, height: 3.0 })]
println("${total(shapes)} ${describe(&Circle { radius: 2.0 })} ${describe(shapes[1])}")
```

```output
9.0 area 12.0 area 6.0
```

A bare `List[Shape]` is the error `trait-used-as-type`.

## Derived implementations

`@derive(...)` writes a trait's implementation from the type's fields.
Derive before writing an `impl` by hand: a derived parser, codec or
checker stays in step with the fields.

| Derive | Gives | Import |
| --- | --- | --- |
| `Eq` | `==` and `!=` | none |
| `Eq, PartialOrd, Ord` | `<`, `>`, `sorted()`, `max` (list all three) | none |
| `Eq, Hash` | use as a map key or set element | none |
| `Debug` | `debug(x)`, and `assert_equal` on the type | none |
| `Default` | `T::default()`, from the field defaults | `std.ops.Default` |
| `Serialize, Deserialize` | JSON `encode` and `decode`, `env_decode`, regex `decode`, HTTP `json` and `set_json`, query and form values | `std.encoding` |
| `MapKey` | a payload-free enum as a JSON object key | `std.encoding` |
| `Cli` | a command-line parser with `--help` ([how-to/cli-tool.md](../how-to/cli-tool.md)) | `std.cli` |
| `Validate` | `value.validated()`, checking the rule facts on its fields ([how-to/configuration.md](../how-to/configuration.md)) | `std.validate` |
| `Arbitrary` | generated inputs for `it_prop` | `std.testing` |

Values copy, so there is no `Clone` to derive. An `@error` enum gets
`Display` and `Error` from its messages ([errors.md](errors.md)).

```hd
@derive(Eq, PartialOrd, Ord, Hash, Debug)
struct Version:
    major: u32
    minor: u32

impl Display for Version:
    fn to_string(&self) -> string: "${self.major}.${self.minor}"

a := Version { major: 1, minor: 2 }
b := Version { major: 1, minor: 10 }
let mut seen: Map[Version, string] = {}
seen[a] = "old"
println("$a < $b: ${a < b}; newest ${[a, b].sorted()[1]}; ${seen.len()} ${debug(a)}")
```

```output
1.2 < 1.10: true; newest 1.10; 1 Version { major: 1, minor: 2 }
```

Implementing `Display` is what lets a type appear in `"$value"`.

**Field facts** configure one derive from the field. `@rename("qty")` and
`@skip()` change the JSON view; `@rename_all(.CamelCase)` on the type
renames every field. `@short('n')` and `@positional` shape a `Cli`
argument, and `@range(1, 99)`, `@length(1, 80)` and the other
`std.validate` rules feed `Validate`. Each fact is imported from the
module of its trait.

```hd
use std.encoding.{Deserialize, MapKey, Serialize, rename, skip}
use std.encoding.json.{decode, encode}
use std.ops.Default

@derive(Eq, Hash, Debug, Serialize, Deserialize, MapKey)
enum Size:
    Small
    Large

@derive(Default, Serialize, Deserialize, Eq, Debug)
struct Order:
    @rename("qty")
    quantity: u32 = 1
    sizes: Map[Size, u32]
    @skip()
    cached_total: u32 = 0

order := Order { ...Order::default(), sizes: {.Large: 2}, cached_total: 7 }
text := encode(order)
println("${debug(Order::default())} $text")
println(debug(decode::[Order](text)))
```

```output
Order { quantity: 1, sizes: {}, cached_total: 0 } {"qty":1,"sizes":{"Large":2}}
Result.Ok(Order { quantity: 1, sizes: {Size.Large: 2}, cached_total: 0 })
```

When one trait must see a field differently, write that one derivation as
a block: `impl Serialize for Session by Structure:` with the member line
`cache = pass` leaves `cache` out of that trait alone (`use
std.meta.Structure`).

## Operators

Operator traits live in `std.ops` and must be imported. The result type is
the associated type `Out`.

```hd
use std.ops.Add

@derive(Eq, Debug)
struct Money:
    cents: i64

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { cents: self.cents + rhs.cents }

println(debug(Money { cents: 250 } + Money { cents: 100 }))
```

```output
Money { cents: 350 }
```

The others follow the same shape: `Sub`/`sub`, `Mul`/`mul`, `Div`/`div`,
`Neg`/`neg`, and `Index`/`index` for `value[key]`.

## Associated types and conversions

A trait may declare a type its implementations choose. Refer to it as
`Self::Name` inside the trait and `T::Name` through a bound.

```hd
use std.convert.From

trait Container:
    type Item
    fn first(&self) -> Self::Item?

impl Container for List[string]:
    type Item = string
    fn first(&self) -> string?: self.get(0)

struct Celsius:
    degrees: f64

struct Fahrenheit:
    degrees: f64

impl From[Celsius] for Fahrenheit:
    fn from(c: Celsius) -> Fahrenheit:
        Fahrenheit { degrees: c.degrees * 9.0 / 5.0 + 32.0 }

boiling := Fahrenheit::from(Celsius { degrees: 100.0 })
println("${debug(["a", "b"].first())} ${boiling.degrees}")
```

```output
Option.Some("a") 212.0
```
