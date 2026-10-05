---
title: Control flow as values
---

# `if`, `match`, and loops produce values, so a name is set once on every path

Where `if` is a statement, you declare an empty variable and assign it in each
branch, and a branch you forget leaves it unset. In hd, `if`, `match`, and
blocks are expressions: `speed :=` takes the value of whichever branch runs, so
there is nothing to forget. A loop can produce a value too: `break value` ends
it early with that value, and the loop's `else` gives the value when it runs to
the end without a `break`. No flag variable, no sentinel. Those are all the loops there are: `for`, and
`while` with a condition. There is no `loop` keyword, so an endless loop is
written `while true:` and left with `break`.

```hd
fn delivery_note(country: string, parcels: List[i32]) -> string:
    speed := if country == "DE": "tomorrow" else: "in 3-5 days"
    oversized := for grams in parcels:
        if grams > 20_000:
            break "a ${grams / 1000} kg parcel goes by freight"  # ← the loop's value
    else:
        "every parcel fits the van"  # ← the value when no break ran
    "Arrives $speed; $oversized"

# Delete `else: "in 3-5 days"` and Run. An `if` without
# `else` has no value when the test is false:
#     void-binding: a binding cannot store a void value

# ── plumbing ──
pub fn main() -> void $ Console:
    println(delivery_note("DE", [1200, 800]))
    println(delivery_note("FR", [1200, 31_000]))
```

```edit
replace:     speed := if country == "DE": "tomorrow" else: "in 3-5 days"
with:     speed := if country == "DE": "tomorrow"
error: void-binding: a binding cannot store a void value
```

```output
Arrives tomorrow; every parcel fits the van
Arrives in 3-5 days; a 31 kg parcel goes by freight
```
