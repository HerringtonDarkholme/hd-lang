---
title: Tests
---

# Tests

The program on the right prices parcels and tests the price in three ways.

## The `tests:` block

```hd example
tests:
    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499)
```

A file keeps its unit tests in one **`tests:` block**. Test code can call
private helpers, and it does not ship in the program.

## Examples, tables, and properties

```hd example
it_prop("shipping never drops below the base", prop=fn!(grams: u16):
    assert(shipping_cents(i32(grams)) >= 499)
)
```

- `it` checks **one example**.
- `it_each` runs one body over a **table** of rows.
- `it_prop` checks a rule on **generated inputs**.

`it_each` and `it_prop` come from `std.testing`, so the block starts with a
`use`.

**Test it.** Press **Test**. `hd test` does not run a script body, so this
page has no top-level `println`. One example, two table rows and one property
give `4 tests passed`.

```hd
fn shipping_cents(weight_grams: i32) -> i32:
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    use std.testing.{it_each, it_prop}

    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499)

    it_each(
        "each full 500 g adds 150",
        [(1500, 649), (2000, 799)],
        body=fn!(row: (i32, i32)): assert_equal(shipping_cents(row._0), row._1),
    )

    it_prop("shipping never drops below the base", prop=fn!(grams: u16):
        assert(shipping_cents(i32(grams)) >= 499)
    )
```

```edit
replace:     if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150
with:     if weight_grams <= 1000: 450 else: 499 + (weight_grams - 1000) / 500 * 150
failure: assertion-failed: actual 450, expected 499
```

```tests
4 tests passed
```
