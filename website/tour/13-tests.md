---
title: Tests
---

# Tests sit next to the code they check, with nothing to install

A file keeps its tests in a `tests:` block. Each `it("name"):` is one test
case, and it may call the file's private functions, so you test the real
helper, not a copy made public for the test. `it_each` runs one case per row
of a table. The assertions come from `std.testing`, and `hd test` runs the
cases, so there is no test framework to pick or configure. Test code is
compiled only for testing, so none of it ships in the program.

```hd
fn shipping_cents(weight_grams: i32) -> i32:  # ← private, and still testable
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    use std.testing.{assert_equal, it_each}

    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499, reason="base rate")

    it_each("each full 500 g over 1 kg adds 150", [(1500, 649), (2400, 799)], body=fn!(row: (i32, i32)):
        let (grams, cents) = row
        assert_equal(shipping_cents(grams), cents, reason="per 500 g")
    )

# Press Test. Then change the first 499, the base rate, to 450
# and press Test again:
#     assertion-failed: base rate: actual 450, expected 499

# ── plumbing ──
```

```edit
replace:     if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150
with:     if weight_grams <= 1000: 450 else: 499 + (weight_grams - 1000) / 500 * 150
failure: assertion-failed: base rate: actual 450, expected 499
```

```tests
3 tests passed
```
