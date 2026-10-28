---
title: Absence and failure
---

# `Option` makes absence explicit, `Result` carries failure, and `?` passes it up

`Option[T]` is the generic enum `.Some(value)` or `.None`; `T?` is its short
spelling. `Result[T, E]` is `.Ok(value)` or `.Err(error)`. Callers handle those
variants explicitly. Postfix `?` takes the success value or returns the same
failure from the current function, keeping the success path straight.

```hd
enum CouponError:
    Unknown(code: string)

fn email_of(name: string) -> string?:
    if name == "Ada": .Some("ada@example.com") else: .None

fn discount(code: string) -> Result[i32, CouponError]:
    if code == "SPRING10": .Ok(10) else: .Err(.Unknown(code))

fn final_cents(cents: i32, percent: i32) -> i32:
    cents * (100 - percent) / 100

fn checkout(cents: i32, code: string) -> Result[i32, CouponError]:
    percent := discount(code)?
    .Ok(final_cents(cents, percent))

println(email_of("Grace").unwrap_or("no email"))
match checkout(5000, "SPRING10"):
    .Ok(due) => println("due: $due")
    .Err(.Unknown(code)) => println("unknown coupon: $code")

# Delete the `?` after `discount(code)` and Run:
#     type-mismatch: in argument: expected i32, found Result[i32, CouponError]
```

```edit
replace:     percent := discount(code)?
with:     percent := discount(code)
error: type-mismatch: in argument: expected i32, found Result[i32, CouponError]
```

```output
no email
due: 4500
```
