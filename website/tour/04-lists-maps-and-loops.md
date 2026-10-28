---
title: Lists, maps, and loops
---

# Collections keep one element type, loops visit them, and comprehensions build them

`[...]` builds a `List`, and `{key: value}` builds a `Map`. Every element has
the collection's type. A `for` loop visits each value. A comprehension puts
its loop and filter before `=>`, then produces one result element. Map lookup
uses `get`, whose optional result needs a fallback when a key may be absent.
Loops may also produce a value with `break value` and a loop `else`; an endless
loop is `while true:`, because hd has no separate `loop` keyword.

```hd
fn count_of(counts: Map[string, i32], name: string) -> i32:
    counts.get(name).unwrap_or(0)

items := ["lamp", "bulb", "lamp"]
let mut counts: Map[string, i32] = {}
for item in items:
    counts[item] = count_of(counts, item) + 1
singletons := [for (name, count) in counts if count == 1 => name]
println("lamps: ${count_of(counts, "lamp")}; single items: ${singletons.len()}")

# Change `count_of` to return `counts.get(name)` and Run:
#     type-mismatch: in result: expected i32, found i32?
```

```edit
replace:     counts.get(name).unwrap_or(0)
with:     counts.get(name)
error: type-mismatch: in result: expected i32, found i32?
```

```output
lamps: 2; single items: 1
```
