---
title: Absence and failure
---

# Absence and failure

The program on the right looks up an email and applies a coupon.

## `Option`

```hd example
fn email_of(name: string) -> string?:
    if name == "Ada": .Some("ada@example.com") else: .None
```

`Option[T]` is either `.Some(value)` or `.None`, and **`T?`** is its short
spelling. The caller must handle both, here with `unwrap_or`.

## `Result`

```hd example
fn discount(code: string) -> Result[i32, CouponError]:
    if code == "SPRING10": .Ok(10) else: .Err(.Unknown(code))
```

`Result[T, E]` is either `.Ok(value)` or `.Err(error)`. Here the error is a
`CouponError`.

## `?`

```hd example
percent := discount(code)?
```

Postfix **`?`** takes the success value. On failure, it returns the same
error from the current function. So the success path stays straight.

## Matching a result

```hd example
match checkout(5000, "SPRING10"):
    .Ok(due) => println("due: $due")
    .Err(.Unknown(code)) => println("unknown coupon: $code")
```

`match` handles each case. A pattern can reach **inside the error**, as
`.Err(.Unknown(code))` does.

**Run it.** Grace has no email, and the coupon takes 10% off 5000. So it
prints `no email` and `due: 4500`.

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
