# How To: Parse A Text File Into Records

**Scenario.** A CSV export of orders must become typed records, with a
clear error that names the line when a row is bad, and then a per-customer
total.

## The program

```hd
use std.encoding.Deserialize
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
use std.regex.{Regex, RegexError}
use std.text.r

# A row's fields are the regex's named groups.
@derive(Deserialize, Eq, Debug)
struct Order:
    customer: string
    item: string
    cents: i64

@error
@derive(Eq, Debug)
pub enum ImportError:              # `pub`: the public `main` returns it
    @error("line $line: expected customer,item,cents")
    Shape(line: usize)
    @error("line $line: $error")
    Field(line: usize, error: RegexError)
    @error("cannot read the file")
    Read(@from error: FsError)

fn parse(text: string) -> Result[List[Order], ImportError]:
    row := Regex::new(r"^ *(?<customer>[^,]*?) *, *(?<item>[^,]*?) *, *(?<cents>[^,]*?) *$").expect("a valid pattern")
    let mut orders: List[Order] = []
    for (index, line) in text.lines().iter().enumerate():
        if index == 0 || line.trim() == "":     # the header, and blank lines
            continue
        match row.decode::[Order](line):        # match, then build an Order from the groups
            .Ok(order) => orders.push(order)
            .Err(.NoMatch) => return .Err(.Shape(index + 1))
            .Err(error) => return .Err(.Field(index + 1, error))
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
        assert_equal(parse("h\nada,lamp\n"), .Err(.Shape(2)))
        result := parse("h\nada,lamp,many\n")
        assert_equal(result.err().map(fn(error): error.to_string()), .Some("line 2: cannot decode regex group cents as i64"))

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

- **Decode, don't split by hand.** `@derive(Deserialize)` on the record,
  and a regex with one named group per field: `row.decode::[Order](line)`
  matches the line and builds the record, parsing `cents` as an `i64`.
  `.Err(.NoMatch)` is a line of the wrong shape; any other `RegexError`
  names the group that did not parse.
- **Compile the pattern once**, outside the loop. Write it as a raw
  string, `r"..."`, so `\d` needs no second backslash.
- **One error enum for the whole job.** `@error` writes the messages, with
  `$name` reading the variant's fields. `@from` on `Read` lets `?` convert
  an `FsError`. Add `@derive(Eq, Debug)` so tests can compare errors.
- **Line numbers** come from `iter().enumerate()`, which counts from 0.
- **Grouping** is a map and `get_or_insert`. `entries()` turns the map
  into a list of pairs to sort.
- **Sorting largest first**: sort by the negated key, or
  `sorted_by_key(...)` then `reversed()`.
- **Money stays an integer** (cents). Format it at the edge.
- **When the text is JSON**, skip the regex: `decode::[List[Order]](text)`
  from `std.encoding.json` reads the same derived record
  ([configuration.md](configuration.md)).
- **Without a regex**: `line.split(",")` gives the fields and
  `field.parse::[i64]()` a `Result`. Neither understands quoted CSV
  fields; for those, walk `line.chars()`.
