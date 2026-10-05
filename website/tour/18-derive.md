---
title: Derive
---

# One `@derive` line gives a type random test inputs, so a property test finds the case you missed

Hand-picked examples test the cases you thought of. A property test states a
rule, here "parsing a printed price gives the price back", and checks it on
many generated values. `@derive(Arbitrary, Debug, Eq)` writes the three impls
the test needs: random `Price` values, printing one when it fails, and
comparing two. When the rule breaks, the test fails with the seed that
reproduces it, and `hd test` also shrinks the failing input to a small one,
such as `Price { cents: -1000 }`. The derivations are written once, at the
type, not per test.

```hd
@derive(Arbitrary, Debug, Eq)  # ← random Prices, printed and compared for free
data Price:
    cents: i32

tests:
    it_prop("parsing a printed price gives it back", prop=fn!(price: Price):
        if price.cents > -2147483648:  # 0 - MIN overflows
            assert_equal(parse(print(price)), .Some(price), reason="round trip")
    )

# Press Test. Then drop the padding: change `pad(abs % 100)` to
# `abs % 100`, so $10.00 prints as "10.0", and press Test again:
#     property test "parsing a printed price gives it back" (seed …) failed

# ── plumbing ──
use std.num.parse_i32
use std.testing.{Arbitrary, assert_equal, it_prop}

fn print(price: Price) -> string:
    sign := if price.cents < 0: "-" else: ""
    abs := if price.cents < 0: 0 - price.cents else: price.cents
    "$sign${abs / 100}.${pad(abs % 100)}"

fn pad(n: i32) -> string:
    if n < 10: "0$n" else: n.to_string()

fn parse(text: string) -> Price?:
    negative := text.starts_with("-")
    body := if negative: text.slice(1, text.len()) else: text
    let .Some((whole, fraction)) = body.split_once(".") else: return .None
    if fraction.len() != 2:
        return .None
    cents := parse_i32(whole).ok()? * 100 + parse_i32(fraction).ok()?
    .Some(Price { cents: if negative: 0 - cents else: cents })
```

```edit
replace:     "$sign${abs / 100}.${pad(abs % 100)}"
with:     "$sign${abs / 100}.${abs % 100}"
failure: property test "parsing a printed price gives it back"
```

```tests
1 test passed
```
