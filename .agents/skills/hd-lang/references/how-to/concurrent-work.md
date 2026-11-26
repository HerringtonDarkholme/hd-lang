# How To: Do Several Waits At Once, With Timeouts And Retries

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

## The pieces

- **`join!` and `race!` take calls without `!`.** `price("tea")` describes
  work that has not started; `join!` starts all of it and waits for all,
  `race!` waits for the first and cancels the rest. Writing
  `join!(price!("tea"), ...)` is a `type-mismatch`, because `price!("tea")`
  already ran.
- **A list of calls** is spread with `...` after the list:
  `join!(calls...)` returns a list of results.
- **A bound on how many wait at once**: `map_join!(items, 2, price)`
  calls `price` on each item with at most 2 in flight, and returns the
  results in item order.
- **Policies** come from `std.task` and compose with `|>`, from the work
  outward: `retry(backoff) |> with_timeout(1s, _)` puts one deadline
  around all attempts. `apply!(policy):` runs the indented block under it.
  The block returns a `Result`, and its error type needs
  `From[TimedOut]` so a deadline can be reported: a `@from` payload of type
  `TimedOut` in an `@error` enum writes that conversion.
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
