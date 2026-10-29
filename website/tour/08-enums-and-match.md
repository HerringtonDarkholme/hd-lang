---
title: Enums and match
---

# Enums and match

The program on the right turns an order's status into a label.

## Enums

```hd example
enum OrderStatus:
    Pending
    Paid(cents: i32)
    Shipped(carrier: string)
```

An `enum` lists the **only possible variants**. A variant may carry data,
like the carrier of a shipped order. Where the type is known, `.Paid(4999)`
is enough to name a variant.

## `match`

```hd example
match status:
    .Pending => "pending"
    .Paid(cents) => "paid $cents cents"
    .Shipped(carrier) => "shipped with $carrier"
```

`match` picks the arm for the variant and **unpacks its data**. It must cover
every variant. So a new variant, such as the commented-out `Refunded`, points
to each `match` that still needs an answer.

**Run it.** It labels one paid and one shipped order, so it prints
`paid 4999 cents` and `shipped with DHL`.

```hd
enum OrderStatus:
    Pending
    Paid(cents: i32)
    Shipped(carrier: string)
    # Refunded(cents: i32)

fn label(status: OrderStatus) -> string:
    match status:
        .Pending => "pending"
        .Paid(cents) => "paid $cents cents"
        .Shipped(carrier) => "shipped with $carrier"

println(label(.Paid(4999)))
println(label(.Shipped("DHL")))
```

```edit
replace:     # Refunded(cents: i32)
with:     Refunded(cents: i32)
error: nonexhaustive-match: no arm matches Refunded
```

```output
paid 4999 cents
shipped with DHL
```
