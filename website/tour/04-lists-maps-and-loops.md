---
title: Lists, maps, and loops
---

# Lists, maps, and loops

The program on the right counts how often each item appears in a list.

## Lists

```hd example
items := ["lamp", "bulb", "lamp"]
```

`[...]` builds a `List`. Every element has the **same type**.

## Maps

```hd example
let mut counts: Map[string, i32] = {}
```

`{key: value}` builds a `Map`, and `{}` is an empty one. `let mut` makes it
**writable**, so the loop can set entries.

## Lookup

```hd example
fn count_of(counts: Map[string, i32], name: string) -> i32:
    counts.get(name).unwrap_or(0)
```

A key may be missing, so `get` returns an **optional** value. `unwrap_or(0)`
gives the fallback.

## `for` loops

```hd example
for item in items:
    counts[item] = count_of(counts, item) + 1
```

`for` visits **each element** in order. `counts[item] = ...` sets one entry.

## Comprehensions

```hd example
singletons := [for (name, count) in counts if count == 1 => name]
```

A comprehension puts its **loop and filter** before `=>`, then the element it
produces. This one keeps the names seen once.

**Run it.** The list has two lamps and one bulb, so it prints
`lamps: 2; single items: 1`.

```hd
fn count_of(counts: Map[string, i32], name: string) -> i32:
    counts.get(name).unwrap_or(0)

items := ["lamp", "bulb", "lamp"]
let mut counts: Map[string, i32] = {}
for item in items:
    counts[item] = count_of(counts, item) + 1
singletons := [for (name, count) in counts if count == 1 => name]
println("lamps: ${count_of(counts, "lamp")}; single items: ${singletons.len()}")
```

```edit
replace: items := ["lamp", "bulb", "lamp"]
with: items := ["lamp", +1, "bulb", "lamp"]
error: no-common-type: in list element: string and an integer literal have no common type
```

```output
lamps: 2; single items: 1
```
