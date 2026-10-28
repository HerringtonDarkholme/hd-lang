---
title: Traits and generics
---

# A trait is a checked contract, and a bound lets one algorithm use it

A `trait` names behavior. An `impl` opts a type into that contract, and the
compiler checks every required method. A generic function works for any `T`
whose bound it names. `total` knows nothing about books or services; it knows
only that every `T < Priced` can return its price. Methods that do not belong
to a trait live in a plain `impl Type:` block.

```hd
trait Priced:
    fn cents(&self) -> i32
    fn label(&self) -> string

struct Book:
    price: i32

struct Service:
    hourly: i32

impl Priced for Book:
    fn cents(&self) -> i32: self.price
    fn label(&self) -> string: "book"

impl Priced for Service:
    fn cents(&self) -> i32: self.hourly
    fn label(&self) -> string: "service"

fn total[T < Priced](items: List[T]) -> i32:
    let sum = +0
    for item in items:
        sum += item.cents()
    sum

println(total([Book { price: 1200 }, Book { price: 800 }]))
println(total([Service { hourly: 5000 }]))

# Delete Book's `cents` method and Run:
#     missing-trait-method: the implementation does not write `cents`
```

```edit
replace:     fn cents(&self) -> i32: self.price
with:
error: missing-trait-method: the implementation does not write `cents`
```

```output
2000
5000
```
