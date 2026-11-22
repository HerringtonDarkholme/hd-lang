# How To: Model A Domain With States And Rules

**Scenario.** An order moves from draft to paid to shipped. Some moves are
illegal, each state carries its own data, and every rule needs a test.

## The program

```hd
@derive(Eq, Debug)
struct Line:
    item: string
    cents: i64
    quantity: u32

# Each state carries only the data that exists in that state.
@derive(Eq, Debug)
enum Order:
    Draft(lines: List[Line])
    Paid(lines: List[Line], total: i64)
    Shipped(total: i64, carrier: string)
    Cancelled(reason: string)

@error
@derive(Eq, Debug)
enum OrderError:
    @error("an order needs at least one line")
    Empty
    @error("line '$item' has no quantity")
    NoQuantity(item: string)
    @error("cannot $action an order that is $state")
    WrongState(action: string, state: string)

fn state_name(order: &Order) -> string:
    match order:
        .Draft(_) => "a draft"
        .Paid(_, _) => "paid"
        .Shipped(_, _) => "shipped"
        .Cancelled(_) => "cancelled"

fn total_of(lines: List[Line]) -> Result[i64, OrderError]:
    if lines.is_empty():
        return .Err(.Empty)
    let total: i64 = 0
    for line in lines:
        if line.quantity == 0:
            return .Err(.NoQuantity(line.item))
        total += line.cents * i64(line.quantity)
    .Ok(total)

# A transition takes the old state and returns the new one, or says why not.
fn pay(order: Order) -> Result[Order, OrderError]:
    match order:
        .Draft(lines) => .Ok(.Paid(lines, total_of(lines)?))
        other => .Err(.WrongState("pay", state_name(&other)))

fn ship(order: Order, carrier: string) -> Result[Order, OrderError]:
    match order:
        .Paid(_, total) => .Ok(.Shipped(total, carrier))
        other => .Err(.WrongState("ship", state_name(&other)))

fn cancel(order: Order, reason: string) -> Result[Order, OrderError]:
    match order:
        .Shipped(_, _) => .Err(.WrongState("cancel", "shipped"))
        _ => .Ok(.Cancelled(reason))

pub fn main() -> void $ Console:
    draft := Order.Draft([Line { item: "lamp", cents: 3200, quantity: 2 }])
    match pay(draft).and_then(fn(paid): ship(paid, "DHL")):
        .Ok(order) => println(debug(order))
        .Err(error) => println("refused: $error")
    match ship(draft, "DHL"):
        .Ok(order) => println(debug(order))
        .Err(error) => println("refused: $error")

tests:
    it("pays a draft and totals its lines"):
        draft := Order.Draft([Line { item: "lamp", cents: 3200, quantity: 2 }, Line { item: "bulb", cents: 450, quantity: 1 }])
        assert_equal(pay(draft).map(fn(order): state_name(&order)), .Ok("paid"))
        match pay(draft):
            .Ok(.Paid(_, total)) => assert_equal(total, 6850)
            _ => panic("expected a paid order")

    it("refuses an empty order and a line with no quantity"):
        assert_equal(pay(.Draft([])), .Err(.Empty))
        assert_equal(pay(.Draft([Line { item: "lamp", cents: 1, quantity: 0 }])), .Err(.NoQuantity("lamp")))

    it("refuses moves from the wrong state"):
        assert_equal(ship(.Draft([]), "DHL"), .Err(.WrongState("ship", "a draft")))
        assert_equal(cancel(.Shipped(100, "DHL"), "late"), .Err(.WrongState("cancel", "shipped")))
        assert_equal(cancel(.Paid([], 0), "changed my mind"), .Ok(.Cancelled("changed my mind")))
```

```output
Order.Shipped(total=6400, carrier="DHL")
refused: cannot ship an order that is a draft
```

## The pieces

- **One enum, one variant per state.** Data that only exists later, such
  as `carrier`, lives on the variant where it exists, so an unpaid order
  cannot have a carrier.
- **Transitions are functions** from the old state to
  `Result[Order, OrderError]`. A `match` names the states that may move,
  and a final arm binds the rest (`other =>`) to refuse them.
- **`match` must cover every state**, so adding a variant makes the
  compiler list each function to revisit (`nonexhaustive-match`).
- **Values, not objects.** `pay(draft)` leaves `draft` unchanged, which
  is why the example can use it again. Keep the current state in a
  `let order = ...` and reassign it: `order = pay(order)?`.
- **Chaining**: `.and_then(fn(paid): ship(paid, "DHL"))` runs the next
  step only on success; inside a `Result` function, `?` does the same.
- **Rules with messages**: an `@error` enum, with `@derive(Eq, Debug)` so
  tests can compare with `assert_equal`.
- For a value with a rule of its own, such as a non-empty name, keep the
  field private and build it through a function that returns a `Result`
  ([language/modules.md](../language/modules.md)).
