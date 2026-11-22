# How To: Parse A Text File Into Records

**Scenario.** A CSV export of orders must become typed records, with a
clear error that names the line when a row is bad, and then a per-customer
total.

## The program

```hd
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.num.ParseNumberError
use std.path.Path

@derive(Eq, Debug)
struct Order:
    customer: string
    item: string
    cents: i64

@error
@derive(Eq, Debug)
pub enum ImportError:              # `pub`: the public `main` returns it
    @error("line $line: expected 3 fields, found $found")
    FieldCount(line: usize, found: usize)
    @error("line $line: bad amount '$text'")
    BadAmount(line: usize, text: string)
    @error("cannot read the file")
    Read(@from error: FsError)

fn parse_row(line: usize, text: string) -> Result[Order, ImportError]:
    fields := text.split(",").map(fn(field): field.trim())
    if fields.len() != 3:
        return .Err(.FieldCount(line, fields.len()))
    cents := match fields[2].parse::[i64]():
        .Ok(cents) => cents
        .Err(_) => return .Err(.BadAmount(line, fields[2]))
    .Ok(Order { customer: fields[0], item: fields[1], cents: cents })

fn parse(text: string) -> Result[List[Order], ImportError]:
    let mut orders: List[Order] = []
    for (index, line) in text.lines().iter().enumerate():
        if index == 0 || line.trim() == "":     # the header, and blank lines
            continue
        orders.push(parse_row(index + 1, line)?)
    .Ok(orders)

fn totals(orders: List[Order]) -> List[(string, i64)]:
    let mut by_customer: Map[string, i64] = {}
    for order in orders:
        *by_customer.get_or_insert(order.customer, 0) += order.cents
    by_customer.entries().sorted_by_key(fn(entry): 0 - entry._1)

fn import!(path: string) -> Result[List[Order], ImportError] $ FsRead:
    text := read_text!(Path(path))?             # an FsError becomes ImportError.Read
    parse(text)

pub fn main!() -> Result[void, ImportError] $ Console + FsRead:
    for (customer, cents) in totals(import!("orders.csv")?):
        println("${customer.pad_end(10)} ${cents / 100}.${(cents % 100).to_string().pad_start(2, '0')}")
    .Ok(())

tests:
    it("parses rows and skips the header and blanks"):
        orders := parse("customer,item,cents\nada, lamp, 3200\n\ngrace,bulb,450\n")
        assert_equal(orders, .Ok([
            Order { customer: "ada", item: "lamp", cents: 3200 },
            Order { customer: "grace", item: "bulb", cents: 450 },
        ]))

    it("names the bad line"):
        assert_equal(parse("h\nada,lamp\n"), .Err(.FieldCount(2, 2)))
        result := parse("h\nada,lamp,many\n")
        assert_equal(result.err().map(fn(error): error.to_string()), .Some("line 2: bad amount 'many'"))

    it("totals by customer, largest first"):
        orders := parse("h\nada,lamp,100\ngrace,bulb,900\nada,desk,300\n").expect("valid rows")
        assert_equal(totals(orders), [("grace", 900), ("ada", 400)])

    it("reads through a memory file system"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            _ := write_text!(Path("orders.csv"), "h\nada,lamp,100\n")
            assert_equal(import!("orders.csv").map(fn(orders): orders.len()), .Ok(1))
```

## The pieces

- **Split, then validate.** `text.lines()` gives the rows, `split(",")` the
  fields, and `parse::[i64]()` a `Result` to turn into your own error.
- **One error enum for the whole job.** `@error` writes the messages, with
  `$name` reading the variant's fields. `@from` on `Read` lets `?` convert
  an `FsError`. Add `@derive(Eq, Debug)` so tests can compare errors.
- **Line numbers** come from `iter().enumerate()`, which counts from 0.
- **Grouping** is a map and `get_or_insert`. `entries()` turns the map
  into a list of pairs to sort.
- **Sorting largest first**: sort by the negated key, or
  `sorted_by_key(...)` then `reversed()`.
- **Money stays an integer** (cents). Format it at the edge.
- For a quoted CSV dialect, write a small field splitter that walks
  `line.chars()`; `split(",")` does not understand quotes.
