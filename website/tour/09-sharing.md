---
title: Sharing with &mut
---

# Two holders see one object only when you write `&mut`

Sometimes several parts of a program must change one thing: two checkout lanes
selling from the same stock, a connection table, a screen's state. A plain
field holds a copy, so each lane would sell the same five units. A field of type
`&mut Stock` holds a reference to one object instead, and `&mut Stock { ... }`
makes that object. Every holder of the reference sees the others' writes, and
`&mut` is not exclusive, so there is no borrow to juggle. A reference keeps its
object alive for as long as anyone holds it, so it can be stored and returned.

```hd
struct Stock:
    units: i32

struct Lane:
    name: string
    stock: &mut Stock  # ← every lane holds the same object

fn sell(lane: Lane, units: i32) -> void $ Console:
    if lane.stock.units < units:
        println("${lane.name}: only ${lane.stock.units} left")
        return
    lane.stock.units -= units  # ← written through the shared reference
    println("${lane.name}: sold $units")

pub fn main() -> void $ Console:
    stock := &mut Stock { units: 5 }  # ← one new object
    front := Lane { name: "front", stock: stock }
    back := Lane { name: "back", stock: stock }
    sell(front, 3)
    sell(back, 3)
    println("left: ${stock.units}")

# Delete `&mut ` from the `stock :=` line and Run. A plain
# value is no reference, so no lane could share it:
#     type-mismatch: in field: expected &mut Stock, found Stock
```

```edit
replace:     stock := &mut Stock { units: 5 }  # ← one new object
with:     stock := Stock { units: 5 }
error: type-mismatch: in field: expected &mut Stock, found Stock
```

```output
front: sold 3
back: only 2 left
left: 2
```
