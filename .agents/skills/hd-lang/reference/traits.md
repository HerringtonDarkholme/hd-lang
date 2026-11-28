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

`@derive(...)` writes a trait's implementation from a type's fields. The
built-in derives, their field facts, and writing a derive template for
your own trait are in [derive](../design/derive.md).

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
