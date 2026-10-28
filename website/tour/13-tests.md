---
title: Tests
---

# Tests live beside code as examples, tables, and properties

A file keeps unit tests in one `tests:` block. `it` checks one example,
`it_each` runs a table, and `it_prop` checks a rule on generated inputs. Test
code can call private helpers and does not ship in the program. Press **Test**:
`hd test` does not run a script body, so this lesson deliberately has no
top-level `println`.

```hd
fn shipping_cents(weight_grams: i32) -> i32:
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    use std.testing.{it_each, it_prop}

    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499, reason="base rate")

    it_each("each full 500 g adds 150", [(1500, 649), (2000, 799)], body=fn!(row: (i32, i32)):
        let (grams, cents) = row
        assert_equal(shipping_cents(grams), cents, reason="per 500 g")
    )

    it_prop("shipping never drops below the base", prop=fn!(grams: u16):
        assert(shipping_cents(i32(grams)) >= 499, reason="at least the base")
    )

# Change the first 499 in `shipping_cents` to 450 and press Test:
#     assertion-failed: base rate: actual 450, expected 499
```

```edit
replace:     if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150
with:     if weight_grams <= 1000: 450 else: 499 + (weight_grams - 1000) / 500 * 150
failure: assertion-failed: base rate: actual 450, expected 499
```

```tests
4 tests passed
```
