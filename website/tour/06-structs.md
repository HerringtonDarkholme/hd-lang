---
title: Structs
---

# Structs

The program on the right changes an order and keeps an older copy.

## Declaring a struct

```hd example
struct Order:
    id: string
    total_cents: i32
```

A `struct` declares a record with named fields. A literal such as
`Order { id: "A-1042", total_cents: 5000 }` gives **every field** a value.
Leaving one out is an error.

## Writable fields

```hd example
let mut order = Order { id: "A-1042", total_cents: 5000 }
order.total_cents += 500
```

`let mut` makes this local value's **fields writable**.

## Values copy

```hd example
saved := order
```

Assignment **copies** the value. Changing `order` later leaves `saved` at 5000.

## Changed copies

```hd example
revised := Order { ...saved, total_cents: 4500 }
```

`...saved` copies the other fields, and `total_cents` gets a **new value**.

**Run it.** `saved` kept the old total, `order` gained 500, and `revised` has
its own. So it prints `saved 5000; current 5500; revised 4500`.

```hd
struct Order:
    id: string
    total_cents: i32

let mut order = Order { id: "A-1042", total_cents: 5000 }
saved := order
order.total_cents += 500
revised := Order { ...saved, total_cents: 4500 }
println("saved ${saved.total_cents}; current ${order.total_cents}; revised ${revised.total_cents}")
```

```edit
replace: let mut order = Order { id: "A-1042", total_cents: 5000 }
with: order := Order { id: "A-1042", total_cents: 5000 }
error: non-reassignable-binding: `order` never changes; bind it with `let mut` to change a field
```

```output
saved 5000; current 5500; revised 4500
```
