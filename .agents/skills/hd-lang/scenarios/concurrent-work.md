# Do Several Waits At Once, With Timeouts And Retries

**Scenario.** A page needs prices from a slow, sometimes busy service:
fetch several at once, retry a busy answer, give up after a deadline, and
test it all without real waiting.

## The program

```hd
use std.task.{Backoff, TimedOut, apply, join, map_join, race, retry, with_timeout}
use std.time.{Clock, ManualClock, Timestamp, ms, s, sleep}

@error
@derive(Eq, Debug)
pub enum FetchError:
    @error("the service is busy")
    Busy
    @error("no answer in time")
    Slow(@from error: TimedOut)              # `@from`: a timeout becomes a FetchError

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

# The same, with at most 2 calls waiting at a time.
fn some_prices!(items: List[string]) -> List[Result[i32, FetchError]] $ Catalog:
    map_join!(items, 2, price)

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
            assert_equal(some_prices!(["tea", "cake", "jam"]), [.Ok(300), .Ok(400), .Ok(300)])

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

## Decisions

The rules for `join!`, `race!`, `map_join!`, policies and `ManualClock`
are in [concurrency](../design/concurrency.md). The choices here:

- **A list of calls, or a fixed few.** `two_prices!` knows its count and
  gets a tuple. `prices!` has a list and spreads it with `...`.
- **A bound with `map_join!`.** `some_prices!` keeps at most 2 requests in
  flight, so a long list does not flood the service.
- **One deadline around all attempts.** `retry(..) |> with_timeout(1s, _)`
  bounds the whole retry, not each try. The `Slow` variant with `@from`
  lets the deadline become a `FetchError`.
- **No hand loop.** The retry is a policy, so the delays use the `Clock`
  and tests need no real waiting.
- **A race against a fallback.** `fastest_price!` cancels whichever side
  loses; here the five-second fallback never wins.
- **The service is a trait**, so the test installs `Flaky`, which fails a
  set number of times and counts its calls.

## Stop on Ctrl-C

Race the work against a `Signals` wait. Whichever finishes first wins, and
the other is cancelled, so the `defer` blocks of the cancelled work still
run.

```hd
use std.host.{ScriptedSignals, Signals}
use std.task.race
use std.time.{Clock, ManualClock, Timestamp, s, sleep}

fn work!(rounds: usize) -> string $ Clock:
    for _ in 0..rounds:
        sleep!(1s)                          # one unit of work
    "done"

fn interrupted!() -> string $ Signals:
    $.use(Signals).wait!(.Interrupt)        # Ctrl-C, or SIGINT
    "stopped"

pub fn run!(rounds: usize) -> string $ Clock + Signals:
    race!(work(rounds), interrupted())

tests:
    fn clock() -> *mut ManualClock:
        ManualClock::new(Timestamp::from_unix_milliseconds(0))

    it("stops on Ctrl-C"):
        $.with(Clock=clock(), Signals=ScriptedSignals::new([.Interrupt])):
            assert_equal(run!(1000), "stopped")

    it("finishes when no signal arrives"):
        $.with(Clock=clock(), Signals=ScriptedSignals::new([])):
            assert_equal(run!(3), "done")
```

- The kinds are `.Interrupt` (Ctrl-C, SIGINT), `.Terminate` (SIGTERM) and
  `.Hangup` (SIGHUP). While a wait for a kind is running, that signal
  completes the wait instead of stopping the program.
- With no wait running, Ctrl-C stops the program as usual.
- `ScriptedSignals::new([...])` delivers each listed signal once, to the
  first wait for its kind. A wait for a kind the list does not hold never
  finishes, so the other side of the race wins.
- A package grants it with `Signals = true` in `[capabilities]`.
