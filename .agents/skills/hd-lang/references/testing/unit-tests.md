# Unit Tests

## The `tests:` block

A file keeps its unit tests in one `tests:` block at the end. Each
`it("name"):` is a case. Test code may call the file's private functions,
and it is left out of the built program.

```hd
fn shipping_cents(weight_grams: i32) -> i32:
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499)

    it("each full 500 g adds 150"):
        assert_equal(shipping_cents(2000), 799, reason="two steps over 1 kg")
        assert(shipping_cents(5000) > 799)
```

```sh
hd test FILE.hd                     # the cases of one file
hd test                             # in a package: every module, tests/, and doc tests
hd test --filter "500 g"            # only the cases whose name contains the text
hd check --tests FILE.hd            # type-check the test code without running it
```

- `it`, `assert` and `assert_equal` need no import inside test code.
- `assert_equal(actual, expected)` needs the type to be `Eq` and `Debug`:
  add `@derive(Eq, Debug)` to your own types.
- `reason="..."` is optional context printed on failure.
- One `tests:` block per file. Helpers, structs and `impl` blocks used only
  by tests go inside it. Its own `use` lines go at its top.
- A test run runs no top-level statements. A file with tests keeps its work
  in functions and `main`, not in a script body.

## Checking errors and optionals

```hd
@derive(Eq, Debug)
enum WithdrawError:
    Low(balance: i32)
    Closed

fn withdraw(balance: i32, amount: i32) -> Result[i32, WithdrawError]:
    if amount > balance: .Err(.Low(balance)) else: .Ok(balance - amount)

tests:
    it("returns the new balance"):
        assert_equal(withdraw(10, 4), .Ok(6))

    it("refuses an overdraft"):
        assert_equal(withdraw(5, 9), .Err(.Low(5)))
        assert(withdraw(5, 9).is_err())

    it("reads the error's payload"):
        match withdraw(5, 9):
            .Err(.Low(balance)) => assert_equal(balance, 5)
            _ => panic("expected Low")

    it("compares an optional"):
        assert_equal([1, 2].get(5), .None)
        assert_equal("a=b".split_once("="), .Some(("a", "b")))
```

## Tables and properties

`it_each` runs one body over a table of rows, and `it_prop` checks a rule
on generated inputs. Both come from `std.testing`, and their bodies are
`fn!` closures.

```hd
fn shipping_cents(weight_grams: i32) -> i32:
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    use std.testing.{it_each, it_prop}

    it_each(
        "each full 500 g adds 150",
        [(1500, 649), (2000, 799)],
        body=fn!(row: (i32, i32)): assert_equal(shipping_cents(row._0), row._1),
    )

    it_prop("shipping never drops below the base", prop=fn!(grams: u16):
        assert(shipping_cents(i32(grams)) >= 499)
    )
```

A property's parameter type picks the generator: numbers, `bool`,
`string`, lists, and your own types with `@derive(Arbitrary)` (import
`Arbitrary` from `std.testing`). `hd test --seed N` repeats a run.

```hd
use std.testing.Arbitrary

@derive(Arbitrary, Debug)
struct Point:
    x: i32
    y: i32

fn norm1(p: Point) -> i64: i64(p.x).abs() + i64(p.y).abs()

tests:
    use std.testing.it_prop

    it_prop("never negative", prop=fn!(p: Point):
        assert(norm1(p) >= 0)
    )
```

## Tests that wait

A case may make bang calls directly.

```hd
use std.time.{Clock, ManualClock, Timestamp, ms, now, sleep}

fn stamp_after!(delay: i64) -> i64 $ Clock:
    sleep!(ms(delay))
    now().unix_milliseconds()

tests:
    it("sleeps on a manual clock without waiting"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Clock=clock):
            assert_equal(stamp_after!(5000), 5000)
```

## Reading a failure

```text
PANIC orders.hd:39: doubles
    panic: assertion-failed: 2 * 2: actual 6, expected 4
    <std>/testing.hd:736 std.testing.check_equal
    orders.hd:40 test "doubles"
    repro: hd test orders.hd --filter "doubles"
test result: FAILED. 2 passed; 1 failed; 0 ignored
```

The second line is the assertion with its `reason`, the `orders.hd:40`
line is where it failed, and the `repro:` command reruns that one case.
