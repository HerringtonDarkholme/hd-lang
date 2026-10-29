---
title: Traits and generics
---

# Traits and generics

The program on the right adds up prices for books and for services.

## Traits

```hd example
trait Priced:
    fn cents(&self) -> i32
    fn label(&self) -> string
```

A `trait` names behavior that **many types** can share.

## Implementations

```hd example
impl Priced for Book:
    fn cents(&self) -> i32: self.price
    fn label(&self) -> string: "book"
```

`impl Priced for Book` opts `Book` into the trait. The compiler checks that
it writes **every required method**. Methods outside any trait go in a plain
`impl Book:` block.

## Generic functions

```hd example
fn total[T < Priced](items: List[T]) -> i32:
    let sum = +0
    for item in items:
        sum += item.cents()
    sum
```

`total` works for any `T` with the **bound** `T < Priced`. It knows nothing
about books or services, only that each item has `cents()`.

**Run it.** The books cost 1200 and 800, so it prints `2000`. Then it prints
`5000` for the service.

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
