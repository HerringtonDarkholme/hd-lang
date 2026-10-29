---
title: Derivation
---

# Derivation

The program on the right reads an order from JSON and compares it.

## `@derive`

```hd example
@derive(Eq, Debug, Serialize, Deserialize)
struct Order:
    id: string
    total_cents: i32
```

`@derive` asks for standard implementations that **follow the fields**. The
compiler describes each field to the derivation code. You only name the
behavior you want.

## The derived behavior

```hd example
match decode::[Order](text):
    .Ok(order) =>
        expected := Order { id: "A-1042", total_cents: 2500 }
        println("same: ${order == expected}")
        println(debug(order))
    .Err(error) => println("decode failed: $error")
```

- `Eq` gives `==`, as in `order == expected`.
- `Debug` gives `debug(order)`, which describes the fields.
- `Serialize` and `Deserialize` connect the type to JSON, so `decode::[Order]`
  can read it.

**Run it.** The decoded order equals the literal, so it prints `same: true`.
Then it prints the order's debug text.

```hd
use std.json.decode
use std.serde.{Deserialize, Serialize}

@derive(Eq, Debug, Serialize, Deserialize)
struct Order:
    id: string
    total_cents: i32

text := "{\"id\": \"A-1042\", \"total_cents\": 2500}"
match decode::[Order](text):
    .Ok(order) =>
        expected := Order { id: "A-1042", total_cents: 2500 }
        println("same: ${order == expected}")
        println(debug(order))
    .Err(error) => println("decode failed: $error")
```

```edit
replace: @derive(Eq, Debug, Serialize, Deserialize)
with: @derive(Eq, Serialize, Deserialize)
error: unsatisfied-trait-bound: Order does not implement Debug
```

```output
same: true
Order { id: "A-1042", total_cents: 2500 }
```
