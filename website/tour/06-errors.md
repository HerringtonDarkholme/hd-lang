---
title: Errors and ?
---

# Errors are values in the return type, and `?` passes them up

A function that can fail returns `Result[T, E]`: `.Ok(value)` or `.Err(error)`.
The failure is in the signature, so no caller is surprised by a hidden
exception. Postfix `?` unwraps an `.Ok`, and on an `.Err` it returns that error
from the current function, so the happy path reads top to bottom. Forget the `?`
and you are holding a `Result`, not a number, and the compiler says so. Real
code uses an error enum instead of `string`; a later page shows how.

```hd
fn checkout(cents: i32, coupon: string, card: string) -> Result[string, string]:
    percent := discount(coupon)?  # ← on .Err, return it to the caller
    receipt := charge(card, cents, percent)?
    .Ok(receipt)

pub fn main() -> void $ Console:
    for coupon in ["SPRING10", "FREESTUFF"]:
        match checkout(5000, coupon, "4242"):
            .Ok(receipt) => println(receipt)
            .Err(reason) => println("checkout failed: $reason")

# Delete the `?` after `discount(coupon)` and Run:
#     type-mismatch: expected i32, found Result[i32, string]

# ── plumbing ──
fn discount(coupon: string) -> Result[i32, string]:
    if coupon == "SPRING10": .Ok(10) else: .Err("unknown coupon $coupon")

fn charge(card: string, cents: i32, percent: i32) -> Result[string, string]:
    due := cents * (100 - percent) / 100
    if card == "0000": .Err("card declined") else: .Ok("charged $due cents to card $card")
```

```edit
replace:     percent := discount(coupon)?  # ← on .Err, return it to the caller
with:     percent := discount(coupon)
error: type-mismatch: expected i32, found Result[i32, string]
```

```output
charged 4500 cents to card 4242
checkout failed: unknown coupon FREESTUFF
```
