---
title: Structs
---

# A struct names its fields, values copy, and `...` makes a changed copy

A `struct` declares a record, and a literal provides each field. `let mut`
makes one local value's fields writable. Assignment copies a value, so `saved`
stays unchanged. `Order { ...saved, field: value }` copies the other fields
while replacing the named one.

```hd
struct Order:
    id: string
    total_cents: i32

let mut order = Order { id: "A-1042", total_cents: 5000 }
saved := order
order.total_cents += 500
revised := Order { ...saved, total_cents: 4500 }
println("saved ${saved.total_cents}; current ${order.total_cents}; revised ${revised.total_cents}")

# Replace `let mut order` with `order :=` and Run:
#     non-reassignable-binding: `order` never changes; bind it with `let mut` to change a field
# Delete `total_cents: 5000` from the first Order and Run:
#     missing-required-field: `total_cents` is required in `Order`
```

```edit
replace: let mut order = Order { id: "A-1042", total_cents: 5000 }
with: order := Order { id: "A-1042", total_cents: 5000 }
error: non-reassignable-binding: `order` never changes; bind it with `let mut` to change a field
```

```output
saved 5000; current 5500; revised 4500
```
