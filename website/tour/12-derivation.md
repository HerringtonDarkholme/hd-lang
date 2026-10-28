---
title: Derivation
---

# Derivation follows a type's structure to provide standard behavior

`@derive` asks standard implementations to follow a type's fields. `Eq` and
`Debug` compare and describe this `Order`; `Serialize` and `Deserialize` make
the same structure available to JSON. The compiler describes each field and
variant to derivation code; ordinary code only names the behavior it wants at
the declaration.

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

# Remove `Debug` from the derive list and Run:
#     unsatisfied-trait-bound: Order does not implement Debug
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
