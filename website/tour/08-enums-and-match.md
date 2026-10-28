---
title: Enums and match
---

# An enum is a closed set of alternatives, and `match` covers every one

An `enum` lists the only possible variants. A variant may carry data, such as
the carrier of a shipped order. `match` selects a variant, unpacks its data,
and must be exhaustive. Adding a business state therefore points to every
decision that still needs an answer.

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

# Uncomment `Refunded` and Run:
#     nonexhaustive-match: no arm matches Refunded
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
