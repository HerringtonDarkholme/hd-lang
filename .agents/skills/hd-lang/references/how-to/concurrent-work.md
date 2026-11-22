# How To: Do Several Waits At Once, With Timeouts And Retries

**Scenario.** A page needs prices from a slow, sometimes busy service:
fetch several at once, retry a busy answer, give up after a deadline, and
test it all without real waiting.

## The program

```hd
use std.convert.From
use std.task.{Backoff, TimedOut, apply, join, race, retry, with_timeout}
use std.time.{Clock, ManualClock, Timestamp, ms, s, sleep}

@derive(Eq, Debug)
pub enum FetchError:
    Busy
    Slow(TimedOut)

impl From[TimedOut] for FetchError:         # lets a timeout become a FetchError
    fn from(error: TimedOut) -> FetchError: .Slow(error)

pub trait Catalog:
    fn price!(&mut self, item: string) -> Result[i32, FetchError]

fn price!(item: string) -> Result[i32, FetchError] $ Catalog:
    $.use(Catalog).price!(item)

# A fixed number of calls together: a tuple of results, in order.
fn two_prices!() -> (Result[i32, FetchError], Result[i32, FetchError]) $ Catalog:
    join!(price("tea"), price("cake"))

# Any number of calls together: build a list of calls, then spread it.
fn prices!(items: List[string]) -> List[Result[i32, FetchError]] $ Catalog:
    join!([for item in items => price(item)]...)

# Retry up to 3 times, 10 ms apart, all within one second.
fn patient_price!(item: string) -> Result[i32, FetchError] $ Catalog + Clock:
    policy := retry(Backoff { attempts: 3, initial: 10ms }) |> with_timeout(1s, _)
    apply!(policy):
        price!(item)

fn fallback_price!() -> Result[i32, FetchError] $ Clock:
    sleep!(5s)
    .Ok(0)

# The first to finish wins; the other is cancelled.
fn fastest_price!(item: string) -> Result[i32, FetchError] $ Catalog + Clock:
    race!(price(item), fallback_price())

tests:
    struct Flaky:
        failures: usize
        calls: usize

    impl Catalog for Flaky:
        fn price!(&mut self, item: string) -> Result[i32, FetchError]:
            self.calls += 1
            if self.calls <= self.failures: .Err(.Busy) else: .Ok(i32(item.len()) * 100)

    it("fetches several prices together"):
        $.with(Catalog=box(Flaky { failures: 0, calls: 0 })):
            assert_equal(two_prices!(), (.Ok(300), .Ok(400)))
            assert_equal(prices!(["tea", "cake", "jam"]), [.Ok(300), .Ok(400), .Ok(300)])

    it("retries a busy answer, on a clock that does not wait"):
        catalog := box(Flaky { failures: 2, calls: 0 })
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Catalog=catalog, Clock=clock):
            assert_equal(patient_price!("tea"), .Ok(300))
        assert_equal(catalog.calls, 3)

    it("gives up after the attempts run out"):
        catalog := box(Flaky { failures: 9, calls: 0 })
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Catalog=catalog, Clock=clock):
            assert_equal(patient_price!("tea"), .Err(.Busy))

    it("takes the first answer of a race"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Catalog=box(Flaky { failures: 0, calls: 0 }), Clock=clock):
            assert_equal(fastest_price!("tea"), .Ok(300))
```

## The pieces

- **`join!` and `race!` take calls without `!`.** `price("tea")` describes
  work that has not started; `join!` starts all of it and waits for all,
  `race!` waits for the first and cancels the rest. Writing
  `join!(price!("tea"), ...)` is a `type-mismatch`, because `price!("tea")`
  already ran.
- **A list of calls** is spread with `...` after the list:
  `join!(calls...)` returns a list of results.
- **Policies** come from `std.task` and compose with `|>`, from the work
  outward: `retry(backoff) |> with_timeout(1s, _)` puts one deadline
  around all attempts. `apply!(policy):` runs the indented block under it.
  The block returns a `Result`, and its error type needs
  `From[TimedOut]` so a deadline can be reported.
- **Other policies**: `timeout(duration)` alone, and
  `fallback(backup_function, policy)` to call a backup when the work
  fails.
- **Rows**: retrying and timing out need `$ Clock`, even with a zero delay.
- **Testing**: install `ManualClock`. Its time moves only when every task
  is waiting, so sleeps and deadlines cost no real time and the order of
  events is the same on every run.
- Tasks take turns: one runs until its next `!` call, then another may
  run. Two tasks never change a value at the same instant, so there are
  no locks to write.
