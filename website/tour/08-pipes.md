---
title: Pipes
---

# A pipe reads in the order things happen, and `_` shows where the value goes

Nested calls read inside out: in `format_cents(with_tax(discounted(2500, 10), 8))`
the first step is in the middle and the last is on the left. The pipe
`value |> step` feeds a value into the next step, so the steps read top to
bottom in the order they run. A step marks the value's slot with `_`, so you
never guess which argument it fills; a bare function name, like
`format_cents`, takes the value as its only argument. A call with no `_` is an
error rather than a guess.

```hd
fn price_label(cents: i32) -> string:
    cents
        |> discounted(_, 10)  # ← `_` is where the value goes
        |> with_tax(_, 8)
        |> format_cents

pub fn main() -> void $ Console:
    println(price_label(2500))
    # The same steps, nested, read inside out:
    println(format_cents(with_tax(discounted(2500, 10), 8)))

# Change `discounted(_, 10)` to `discounted(10)` and Run. A step never
# guesses where the value goes:
#     pipe-step-needs-placeholder: a pipe step other than a bare name or path needs '_' to mark the piped value, as in 'f(_, y)'

# ── plumbing ──
fn discounted(cents: i32, percent: i32) -> i32:
    cents * (100 - percent) / 100

fn with_tax(cents: i32, percent: i32) -> i32:
    cents + cents * percent / 100

fn format_cents(cents: i32) -> string:
    "$${cents / 100}.${cents % 100}"
```

```edit
replace:         |> discounted(_, 10)  # ← `_` is where the value goes
with:         |> discounted(10)
error: pipe-step-needs-placeholder: a pipe step other than a bare name or path needs '_' to mark the piped value, as in 'f(_, y)'
```

```output
$24.30
$24.30
```
