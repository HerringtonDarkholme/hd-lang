# The standard library by job

This page lists the std calls by what you want to do: the `use` line, the calls agents reach for, one
checked example, and the mistakes that cost a compile. For full signatures, open the module's file in
[reference/std/](reference/std/README.md), as `reference/std/text.txt`, or search them all with
`grep -rh 'split' reference/std/`.
Inside a package, `hd doc std.text.split` prints one item and `hd doc std.fs` a whole module.

Methods of `string`, `List`, `Map`, `Option` and `Result` come with the type. Everything else is
`use std.MODULE.name` or `use std.MODULE.{a, b}`. Import a bang function by its plain name and call it with
the bang: `use std.fs.read_text`, then `read_text!(path)`. A missing import usually gets a message that
names the line to add, as in ``no trait named `Clock`; add `use std.time.Clock` ``.

## The prelude

These need no import.

| Kind | Names |
| --- | --- |
| Types | `bool`, `i8` to `i64`, `u8` to `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `never`, `List`, `Map`, `Option`, `Result`, `Ordering`, `Any`, `ConsoleError` |
| Traits | `Display`, `Debug`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Copy`, `Iterator`, `Iterable` |
| Capabilities | `Console` |
| Functions | `println`, `panic`, `debug`, `dbg`, `box` |
| In test code only | `it`, `assert`, `assert_equal` |

`@derive`, `@error` and `@from` need no import. Floats provide `sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
`atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`, `exp`, `exp2`, `exp_m1`, `ln`, `log2`, `log10`, `ln_1p`,
`cbrt`, `hypot`, `mul_add`, `powf`, `powi`, `abs` and `signum` without one; angles are radians.
`f64::pi()`, `f64::e()`, `f32::pi()` and `f32::e()` are associated functions. A function or variable named
`string`, `f32` or `println` is `prelude-name-shadow`.

Agents most often forget these imports. Each is `unknown-name`, `unknown-trait` or `unknown-method` without it.

| You wrote | Add |
| --- | --- |
| `max(a, b)`, `min(a, b)`, `clamp(x, lo, hi)` | `use std.cmp.{clamp, max, min}` |
| `sleep!(1ms)`, `now()`, `5ms` calls | `use std.time.{Clock, ms, now, s, sleep}` |
| `defer(fn() ...)` | `use std.ops.defer` |
| `F < Fn() -> i32`, `FnMut` bounds | `use std.ops.{Fn, FnMut}` |
| `Set`, `Deque`, `Heap`, `counts(xs)` | `use std.collections.{Deque, Heap, Set, counts}` |
| `char::try_from(n)`, `u8::try_from(n)` | `use std.convert.TryFrom` |
| `$ FsRead`, `$ FsWrite`, `$ Env`, `$ Net` | the trait, from `std.fs`, `std.host`, `std.net` |
| `@derive(Cli)`, `@short('n')` | `use std.cli.{Cli, short}` |
| `@derive(Serialize, Deserialize)` | `use std.encoding.{Deserialize, Serialize}` |
| `@range(1, 9)`, `@email()` | `use std.validate.{range, email}` |

A derive imports its trait, and each field fact its constructor, from the trait's module. The prelude
traits need none.

## Text and strings

Module `std.text`. A `string` is immutable UTF-8. Every position a method takes or returns is a **byte** offset.

| Call | Result |
| --- | --- |
| `s.len()`, `s.is_empty()` | byte count `usize`; `bool` |
| `s.contains(t)`, `starts_with(t)`, `ends_with(t)` | `bool` |
| `s.find(t)`, `s.rfind(t)` | first or last byte offset, `usize?` |
| `s.slice(start, end)`, `s.replace(old, new)`, `s.repeat(n)` | `string`; `slice` takes bytes `start` up to `end` |
| `s.trim()`, `trim_start()`, `trim_end()`, `lower()`, `upper()` | `string` |
| `s.pad_start(width)`, `s.pad_end(width)` | a second argument is the fill `char` |
| `s.strip_prefix(t)`, `s.strip_suffix(t)`, `s.split_once(sep)` | `string?`; `(string, string)?` |
| `s.split(sep)`, `s.split_whitespace()`, `s.lines()` | `List[string]`; `split` keeps empty pieces |
| `s.chars()`, `s.char_indices()`, `s.bytes()` | iterators of `char`, `(usize, char)`, `u8` |
| `s.to_utf8()`, `string::from_utf8(bytes)` | `List[u8]`; `Result[string, Utf8Error]` |
| `s.parse::[T]()` | `Result[T, ...]` for numbers, `bool`, `char`, `Timestamp` |
| `list.join(sep)`, `a + b`, `"${expr}"`, `n.to_string()` | join, concatenate, interpolate, convert |

`char` methods: `is_alphabetic()`, `is_alphanumeric()`, `is_uppercase()`, `is_ascii_digit()`,
`is_whitespace()`, `to_digit(radix)`, `to_u32()`. A `chars` iterator `collect()`s into a `string`.

```hd
use std.text.StringBuilder

tests:
    it("counts bytes and characters"):
        word := "héllo, world"
        assert_equal(word.len(), 13)                    # bytes
        assert_equal(word.chars().count(), 12)          # characters
        assert_equal(word.find(","), .Some(6))          # a byte offset too
        assert_equal(word.split_once(", "), .Some(("héllo", "world")))
        assert_equal("7".pad_start(3, '0'), "007")
        assert_equal("a,b,,c".split(",").len(), 4)
        assert_equal("  9000 ".trim().parse::[i32](), .Ok(9000))
        assert_equal("many".parse::[i32]().unwrap_or(8080), 8080)
        assert_equal(string::from_utf8([104, 105]), .Ok("hi"))
    it("builds text"):
        let mut out = StringBuilder::new()
        out.push("a".pad_end(3))
        out.push_char('|')
        assert_equal(out.build(), "a  |")
```

- `len`, `slice`, `find` and `char_indices` count bytes. A slice that cuts a character in two panics. Take
  offsets from `find` or `char_indices`, never from counting `chars()`.
- `s[0]` is `type-mismatch`: write `s.slice(0, 1)`, or walk `s.chars()`. hd has no byte literal.
- `i32(text)` is `not-callable`: write `text.parse::[i32]()`. Its error is `std.num.ParseNumberError`.
- `to_uppercase`, `size()`, `length` and `append` do not exist: write `upper()`, `len()`, `push`.
- `println("{}", n)` is `argument-count`: print one value, with `"$n"`.
- Build long output with a `StringBuilder`. `s = s + x` in a loop copies `s` each time.

## Regex and typed decode

Module `std.regex`. A pattern compiles once and runs in linear time. Write it as a raw string `r"..."`
(`use std.text.r`) so `\d` needs no second backslash. To pull fields out of a line, name the groups after the
fields of a `@derive(Deserialize)` struct and call `decode::[T]`: it matches, then parses each group as its
field's type.

```hd
use std.encoding.Deserialize
use std.regex.Regex
use std.text.r

@derive(Deserialize, Debug)
struct Hit:
    method: string
    path: string
    status: i32

fn hit_of(line: string) -> Hit?:
    pattern := Regex::new(r"\"(?<method>[A-Z]+) (?<path>[^ ]*)\" (?<status>\d\d\d)").expect("a valid pattern")
    pattern.decode::[Hit](line).ok()

println("${debug(hit_of("\"GET /ok\" 200"))} ${debug(hit_of("garbage"))}")
```

```output
Option.Some(Hit { method: "GET", path: "/ok", status: 200 }) Option.None
```

`decode` returns `RegexError.NoMatch` when the line does not match, and `WrongType` naming a group that does
not parse. A `T?` field is `.None` when its group took no part. `pattern.captures(line)` gives `get(1)` or
`name("status")`, each a `Match?` with `.text`. `is_match`, `find`, `find_all`, `replace_all` and `split` cover the rest.

## Lists

Module `std.collections.list`; the type needs no import.

| Call | Does |
| --- | --- |
| `xs.len()`, `first()`, `last()`, `get(i)` | size; `T?`. `xs[i]` panics out of range |
| `xs.contains(&v)`, `xs.index_of(v)` | by value; `index_of` gives `usize?` |
| `xs.slice(a, b)`, `take(n)`, `reversed()`, `unique()`, `sorted()` | a new list |
| `xs.min()`, `xs.max()`, `xs.sum()` | `T?` for `Ord` items; a total |
| `xs.map(f)`, `filter(f)`, `flat_map(f)`, `partition(f)` | a new list, eagerly; two lists |
| `xs.any(f)`, `all(f)`, `find(f)`, `zip(ys)`, `windows(n)`, `chunks(n)` | search; pairs and runs |

These change the list and need `let mut xs` or a `&mut List[T]`: `push(v)`, `extend(ys)`, `insert(i, v)`,
`pop()` (`T?`), `remove(i)` (the element), `xs[i] = v`, `swap(i, j)`, `sort()`, `sort_by(f)`, `sort_by_key(f)`,
`reverse()`, `dedup()`, `retain(f)`, `truncate(n)`, `clear()`.

```hd
tests:
    it("sorts and slices"):
        let mut xs = [3, 1, 2]
        xs.sort()                                       # returns void
        assert_equal(xs, [1, 2, 3])
        xs.sort_by(fn(a, b): b.cmp(a))                  # descending
        assert_equal(xs, [3, 2, 1])
        assert_equal(xs.get(9), .None)
        assert_equal(xs.index_of(2), .Some(1))
        assert_equal([1, 2].zip(["a", "b"]), [(1, "a"), (2, "b")])
        assert_equal([for x in 0..10 if x % 2 == 0 => x].len(), 5)
```

- `sort()` and `reverse()` return `void`: `ys.sort()` as the last line of a `-> List[i32]` function is
  `missing-return-value`. Use `sorted()` or `reversed()` for a copy.
- Callbacks borrow the element, as `&T`; leave the parameter type off. A comprehension is
  `[for x in xs if p(x) => f(x)]`.
- A list is a value: passing or assigning it copies it. Pass `&xs` to read, `&mut xs` to change
  ([reference/references.md](reference/references.md)).
- A literal such as `[1, 2, 3]` may infer `usize` when a `usize` method such as `windows` uses it. Write
  `let xs: List[i32] = [...]` when you mean another type.

## Maps and sets

`Map` (`std.collections.map`) needs no import. `Set` (`std.collections.set`) and `counts` come from
`use std.collections.{Set, counts}`. Maps and sets keep insertion order.

| Call | Does |
| --- | --- |
| `m.len()`, `m.contains_key(k)`, `m.get(k)` | size; membership; `V?`. `m[k]` panics when missing |
| `m[k] = v` | insert or replace (needs `let mut`) |
| `m.insert(k, v)`, `m.remove(k)` | the old value `V?`; it must be used, so write `_ := m.insert(k, v)` |
| `m.get_or_insert(k, default)` | `&mut V`, inserting the default when missing |
| `m.keys()`, `m.values()` | iterators; `collect()` into a typed `let` |
| `m.entries()` | a `List[(K, V)]` of copies; there is no `items()` |
| `for (k, v) in m` | each entry, as `(&K, &V)` |
| `s.insert(v)`, `s.remove(v)`, `s.contains(&v)` | `bool`; membership takes a reference |
| `a.union(&b)`, `a.intersection(&b)` | lazy iterators; `collect()` into a `Set` |

```hd
use std.collections.Set

tests:
    it("maps"):
        let mut ages: Map[string, i32] = {}
        ages["ada"] = 36
        *ages.get_or_insert("bob", 0) += 1              # count or update
        assert_equal(ages["bob"], 1)
        let names: List[string] = ages.keys().collect()
        assert_equal(names, ["ada", "bob"])             # insertion order
        assert_equal(ages.entries()[0], ("ada", 36))
    it("sets"):
        let mut seen: Set[string] = Set::new()
        assert(seen.insert("a"))                        # true: new
        assert(!seen.insert("a"))
        let mut other: Set[string] = Set::new()
        _ := other.insert("a")
        let both: Set[string] = seen.intersection(&other).collect()
        assert_equal(both.len(), 1)
        assert(seen.contains(&"a"))
```

## Deque and heap

`use std.collections.{Deque, Heap}` (modules `std.collections.deque` and `std.collections.heap`). A `Deque[T]` has `push_back`, `push_front`, `pop_front`, `pop_back`, `front`,
`back`, `get(i)` and `len`; pops and peeks give `T?`. Use it for a queue or breadth-first search. A `Heap[T]`
needs `Ord` and is a max-heap: `push`, `pop` (the largest), `peek`, `len`, and `to_sorted_list()` (ascending).
Push `Reverse { value: x }` from `std.cmp` for a min-heap.

```hd
use std.collections.{Deque, Heap}

tests:
    it("queues and heaps"):
        let mut queue: Deque[i32] = Deque::new()
        queue.push_back(1)
        queue.push_front(0)
        assert_equal(queue.pop_front(), .Some(0))
        let mut heap: Heap[i32] = Heap::new()
        heap.push(5)
        heap.push(9)
        assert_equal(heap.pop(), .Some(9))
        assert_equal(heap.to_sorted_list(), [5])
```

## Iterators and adapters

`xs.iter()`, `m.iter()`, a range as `(1..=5).iter()`, and `s.chars()` give lazy iterators. The `std.iter` types
(`Filter`, `Take`, `Rev`, ...) are what adapters return; you rarely name them.

| Adapter (lazy) | Gives |
| --- | --- |
| `map(f)`, `filter(f)`, `flat_map(f)`, `filter_map(f)` | changed or kept items; `filter_map` keeps present results |
| `enumerate()`, `zip(other)`, `chain(other)` | `(index, item)` pairs; paired; appended |
| `take(n)`, `skip(n)`, `take_while(f)`, `rev()` | part of the items; reversed, on double-ended cursors |

Draining calls: `collect()` builds the collection named by the expected type (`char` items build a `string`);
`count()`, `last()` (`T?`), `for_each(f)`, `sum()`, `product()`, `fold(init, f)`, `any(f)`, `all(f)`, `find(f)`,
`position(f)` (the remaining index) and `min()`, `max()` (`T?` for `Ord` items: `min` the first of equal ones,
`max` the last).

`xs.windows(n)` and `xs.chunks(n)` are list methods returning a `List` of views, not iterators.
`text.chars().rev().collect::[string]()` reverses by Unicode scalar. `next_back` needs
`use std.iter.DoubleEndedIterator`. Map entries and `from_fn` cursors are forward only.

```hd
tests:
    it("chains adapters"):
        let xs: List[i32] = [1, 2, 3, 4, 5]
        assert_equal(xs.windows(2).map(fn(w): w[1] - w[0]), [1, 1, 1, 1])
        assert_equal(xs.chunks(2).map(fn(c): c.len()), [2, 2, 1])
        let evens: List[i32] = xs.iter().filter(fn(n): n % 2 == 0).collect()
        assert_equal(evens, [2, 4])
        assert_equal(xs.iter().position(fn(n): n == 3), .Some(2))
        assert_equal(xs.iter().skip(3).enumerate().last(), .Some((1, 5)))
        assert_equal((1..=4).iter().fold(0, fn(sum, n): sum + n), 10)
        let index: Map[string, usize] = ["a", "bb"].iter().map(fn(name): (name, name.len())).collect()
        assert_equal(index["bb"], 2)
```

## Counting, grouping, sorting

`counts(words)` gives a `Map[T, usize]`; `.most_common(n)` on it gives the top `n` as `List[(T, usize)]`, largest
first. `xs.group_by(fn(x): key)` gives `Map[K, List[T]]`. Sort by a field with `xs.sorted_by_key(fn(x): x.field)`
(the key must be `Ord`), descending with `Reverse { value: x.field }`, and by two keys with
`xs.sorted_by(fn(a, b): a.k1.cmp(&b.k1).then(b.k2.cmp(&a.k2)))`. `max_by_key` and `min_by_key` give `T?`.

```hd
use std.cmp.Reverse
use std.collections.counts

struct Entry:
    name: string
    score: i32

tests:
    it("counts and ranks"):
        tally := counts("b a c a b a".split_whitespace())
        assert_equal(tally.most_common(2), [("a", 3), ("b", 2)])
        rows := [Entry { name: "x", score: 3 }, Entry { name: "y", score: 9 }, Entry { name: "z", score: 3 }]
        assert_equal(rows.sorted_by_key(fn(e): Reverse { value: e.score })[0].name, "y")
        assert_equal(rows.group_by(fn(e): e.score)[3].len(), 2)
        let mut ordered = rows
        ordered.sort_by(fn(a, b): b.score.cmp(&a.score).then(a.name.cmp(&b.name)))
        assert_equal(ordered.map(fn(e): e.name).join(""), "yxz")
```

`sorted_by_key` lends `&T` to the key function; `sorted_by` and `sort_by` take two `&T` and return `Ordering`.

## Numbers and math

Modules `std.num` and `std.cmp`. Integer and float methods need no import. Integers never convert implicitly.

```hd
use std.cmp.{clamp, max, min}
use std.convert.TryFrom

tests:
    it("numbers"):
        assert_equal(u32::parse_radix("ff", 16), .Ok(255))
        let n: i64 = 300
        assert(u8::try_from(n).is_err())                # narrowing can fail
        assert_equal(i64(7u8), 7)                       # widening cannot
        assert_equal(3.14159.to_fixed(2), "3.14")
        assert_equal(i32(7) / 2, 3)                     # integer division truncates
        assert_equal(i32(-7) % 3, -1)                   # the sign follows the dividend
        assert_equal(i32(2147483647).checked_add(1), .None)
        assert_equal(max(2, 7) + min(2, 7), 9)
        assert_equal(clamp(15, 0, 10), 10)
        assert_equal(i32(3).pow(2), 9)
        assert_equal(3.0.hypot(4.0), 5.0)
        assert_equal(1.0.exp(), 2.718281828459045)      # musl's libm, bit for bit
        assert_equal(2.0.mul_add(3.0, 1.0), 7.0)        # 2 * 3 + 1, rounded once
```

- Widen with `i64(x)`. Narrow with `T::try_from(x)`, a `Result` that needs `std.convert.TryFrom`.
- Overflow panics. Use `checked_add`, `wrapping_add` or `saturating_add` (also `_sub`, `_mul`) when expected.
- Also on integers: `abs`, `abs_diff`, `pow`, `count_ones`, `to_radix`; on floats: `sqrt`, `floor`, `ceil`,
  `round`, `is_nan`, `to_fixed(digits)`. `Integer` and `Float` bound generic numeric code.
- Float math (`sin` to `mul_add` above) gives musl libm's result bit for bit at both widths, so a program
  prints the digits C and Rust print on musl. `exp_m1` and `ln_1p` stay accurate near zero. There is no
  `gamma`, `erf`, `frexp` or Bessel function in std.
- `a.cmp(&b)` gives `Ordering` (`.Less`, `.Equal`, `.Greater`); `.then(other)` chains keys.
- `let n: i64 = 300` is the typed binding. `n: i64 := 300` is `missing-let`.

## Console input and output

Module `std.console`. `println(x)` and `print(x)` (no newline) write to standard output, `eprintln(x)` to standard error: all
`$ Console`. `read_line!()` gives `Result[string?, ConsoleError]` without the line ending, `.Ok(.None)` at the
end of input. `read_all!()` gives all of standard input. Both need `$ ConsoleInput`. Tests use `ScriptedInput`
and `MemoryConsole`. A filter over lines is in [scenarios/stdin-filter.md](scenarios/stdin-filter.md).

## Files and paths

`use std.fs.{...}` and `use std.path.Path`. Every call returns a `Result` with `FsError`. Reads need
`$ FsRead` and writes `$ FsWrite`; `hd.toml` grants the folders ([design/capabilities.md](design/capabilities.md)).

| Call | Needs |
| --- | --- |
| `read_text!(p)`, `read_bytes!(p)`, `list_dir!(p)`, `walk!(p)`, `glob!(pattern)`, `stat!(p)` | `FsRead` |
| `write_text!(p, text)`, `append_text!(p, text)`, `create_dir_all!(p)`, `remove!(p)`, `rename!(a, b)`, `copy!(a, b)` | `FsWrite` |

`read_text!` gives `string`, `stat!` an entry or `.None` when missing, and a write gives `void`.
`Path("reports").join("q4.txt")`, `parent()`, `file_name()`, `file_stem()`, `extension()` and
`with_extension(ext)` build and split paths; `"$path"` gives the text. A write does not create the folder: call
`create_dir_all!` first. Tests use `MemoryFs`. A complete program is in
[scenarios/files-and-directories.md](scenarios/files-and-directories.md).

## Arguments and environment

`args()` from `std.cli` (`$ Args`) is the `List[string]` after `--`, as in `hd greet.hd -- Ada`. `env("HOME")`
from `std.host` is a `string?`, and `env_as::[u16]("PORT")` is `Result[u16?, HostError]` (both `$ Env`).
For options, flags and `--help`, derive `Cli` and call `parse_args()`: see
[scenarios/cli-tool.md](scenarios/cli-tool.md).

`env_decode::[T](prefix="APP_")` fills a `@derive(Deserialize)` struct from the environment. Each field reads
the prefix plus its uppercase name. A `List` field splits on commas, a `T?` field is `.None` when unset, and a
default fills a missing variable. Tests supply `ScriptedEnv`. Example:
[scenarios/configuration.md](scenarios/configuration.md).

A bad value is an error that names the variable, as in `invalid value for APP_PORT: invalid digit at position 0`.

## Processes

`use std.process.{Command, Process}`, `$ Process`. `Command::new("git")` takes `.arg(x)`, `.args(list)`,
`.env(name, value)`, `.current_dir(Path)` and `.stdin(text)`, each returning the command. `.run!()` gives
`Result[ProcessOutput, ProcessError]` with `stdout`, `stderr` and `status`. A non-zero exit is still `.Ok`;
`output.ok()` turns it into `.Err(.Exited(output))`. There is no shell. `ExitCode(n)` is a `main` return
value. Tests use `ScriptedProcess`. See [scenarios/run-commands.md](scenarios/run-commands.md).

## Time and durations

`use std.time.{...}`. `Timestamp` is a point in UTC, `Duration` a length, `Instant` a reading for measuring,
`Date` a calendar date. Literals are `5ms`, `30s`, `90min`, `2h`; calls are `ms(n)`, `s(n)`, `minutes(n)`,
`h(n)`. `now()`, `monotonic()` and `sleep!(d)` need `$ Clock`; tests use `ManualClock`. `elapsed(start)` is a
`Duration`. `Timestamp` has `to_rfc3339()`, `unix_milliseconds()`, `date()`, `format(layout)`.

```hd
use std.time.{Timestamp, ms, s}

tests:
    it("does time arithmetic"):
        fixed := "2026-10-06T12:00:00Z".parse::[Timestamp]().expect("a valid time")
        assert_equal((fixed + 30s).to_rfc3339(), "2026-10-06T12:00:30Z")
        assert_equal(fixed.unix_milliseconds(), 1791288000000)
        assert(500ms < 1s)
```

## Random

`use std.random.{Random, rng}`, `$ Random`. `rng()` gives an `Rng` with `int(1..=6)`, `float()`, `bool()`,
`choose(items)` (`T?`), `shuffle(items)`, `sample(items)` and `next_u64()`. `Rng::new(seed)` and `SeededRandom`
repeat the same sequence; `std.random.fill(n)` gives `n` random bytes.

## JSON and encodings

`use std.encoding.json.{decode, encode, parse}` and `use std.encoding.{Deserialize, Serialize}`. Derive both,
then `decode::[T](text)` gives `Result[T, JsonError]` and `encode(value)` gives text; `encode_pretty` indents. A
missing field is an error naming its path, an extra field is ignored, and a field with a default may be left
out. `parse(text)` gives an untyped `Json` with `get(key)`, `at(i)`, `pointer("/a/0")`, `as_i64()`, `as_text()`:
use it only when the shape is unknown. Field facts change the JSON view: `@rename("totalCents")`,
`@rename_all(.CamelCase)`, `@skip()`, `@skip_none()` ([design/derive.md](design/derive.md)).

```hd
use std.encoding.json.{decode, encode, parse}
use std.encoding.{Deserialize, Serialize}

@derive(Serialize, Deserialize, Debug)
struct Limits:
    retries: i32
    timeout_ms: i32 = 250

tests:
    it("round-trips json"):
        limits := decode::[Limits]("{\"retries\": 3}").expect("valid")
        assert_equal(encode(limits), "{\"retries\":3,\"timeout_ms\":250}")
        assert_equal(decode::[Limits]("{}").err().expect("missing").to_string(), "missing field at $.retries")
        tree := parse("{\"items\": [{\"id\": 7}]}").expect("valid JSON")
        assert_equal(tree.pointer("/items/0/id").and_then(fn(id): id.as_i64()), .Some(7))
```

`std.encoding.base64`: `Base64` has the variants `Standard`, `StandardNoPad`, `Url`, `UrlNoPad`, each with
`encode(bytes) -> string` and `decode(text) -> Result[List[u8], DecodeError]`. `std.encoding.hex` has
`hex_encode(bytes)` and `hex_decode(text)`.

```hd
use std.encoding.base64.Base64
use std.encoding.hex.{hex_decode, hex_encode}

tests:
    it("encodes bytes"):
        assert_equal(Base64.Standard.encode("hi!".to_utf8()), "aGkh")
        assert_equal(Base64.Url.decode("aGkh"), .Ok([104, 105, 33]))
        assert_equal(hex_encode([222, 173]), "dead")
        assert_equal(hex_decode("dead"), .Ok([222, 173]))
```

## Formatting

Module `std.format`. `Display` writes the text of `"$x"`; `Debug` is what `debug(x)` and `dbg(x)` print. Both are
prelude traits and `@derive(Debug)` covers most types. Write `Debug` by hand to reshape or hide a field, with the
`DebugWriter` builders `debug_struct`, `debug_tuple`, `debug_list` and `debug_map`. A builder needs `let mut`.

```hd
use std.format.{Debug, DebugWriter}

struct Secret:
    token: string

impl Debug for Secret:
    fn debug(&self, out: *mut DebugWriter) -> void:
        let mut view = out.debug_struct("Secret")
        view.field("token", "***")
        view.finish()

tests:
    it("hides a field"):
        assert_equal(debug(Secret { token: "x1" }), "Secret { token: \"***\" }")
```

## HTTP and networking

`std.http` has `get!(url)` and `post!(url, value)` (`$ Http`), `Client`, `Request`, `Response`, `Headers`,
`Url`, `Method`, `HttpStatus`, `Cookie`; tests use `ScriptedHttp`. A server and a client are in
[scenarios/http-api.md](scenarios/http-api.md). `std.net` is raw sockets, `$ Net`: `connect!(host, port)`,
`listen!`, `bind_udp!`, `lookup!(host)`. A stream has `read!(max)`, `write!(bytes)`, `close()`. Tests use
`ScriptedNet` with `ScriptedStream`.

```hd
use std.net.{Net, NetError, ScriptedNet, ScriptedStream, connect}

fn ping!() -> Result[string, NetError] $ Net:
    let stream = connect!("example.test", 7)?
    stream.write!("ping".to_utf8())?
    .Ok(string::from_utf8(stream.read!(64)?).unwrap_or(""))

tests:
    it("talks to a scripted server"):
        wire := ScriptedStream::new(["pong".to_utf8()])
        net := ScriptedNet::new()
        net.add_connection("example.test:7", wire)
        $.with(Net=net):
            assert_equal(ping!().ok(), .Some("pong"))
        assert_equal(wire.written(), "ping".to_utf8())
```

## Tasks, threads and sync

Concurrent work uses `!` functions with `join!`, `race!` and `map_join!`. `std.task` adds `retry`, `timeout`,
`with_timeout`, `fallback`, `Backoff` and channels; policies are in [design/concurrency.md](design/concurrency.md).
`std.thread` runs closures on real threads: `scope(fn(s: &mut Scope) -> T: ...)` waits for every thread,
`s.spawn(fn() -> T: ...)` returns a `Handle`, `handle.join()` gives the value, and `background!(fn)` runs one
closure and waits. A thread may borrow a `Sync` struct. A `List` is not `Sync` and cannot cross.
`std.sync.Arc` shares an owned value between threads: `Arc::new(v)`, `a.clone()`, `a.get()`,
`Arc::strong_count(&a)`, and `Weak` through `Arc::downgrade(&a)`.

```hd
use std.sync.Arc
use std.thread.{Scope, scope}

struct Point:
    x: i32
    y: i32

fn total(point: &Point, base: Arc[i32]) -> i32:
    scope(fn(s: &mut Scope) -> i32:
        mine := base.clone()
        left := s.spawn(fn() -> i32: point.x * 2)
        right := s.spawn(fn() -> i32: *mine.get() + point.y)
        left.join() + right.join()
    )

println(total(&Point { x: 1, y: 2 }, Arc::new(10)))
```

```output
14
```

Shared mutable state between tasks: [scenarios/shared-state.md](scenarios/shared-state.md).

## Crypto and hashing

`std.crypto.sha256` and `std.crypto.sha512` give `sha256(bytes) -> Digest` and a hasher:
`Sha256::new()`, `Sha256::hmac(key)`, `.update(bytes)` (chains), `.finish()`. A `Digest` has `to_hex()`.
`std.crypto.aes` is `Aes::new(key)` (16, 24 or 32 bytes). `std.crypto.cipher` builds `Gcm::new(aes)` (`seal`,
`open`) and `Ctr` on it. `std.crypto.rand.fill(n)` gives `n` secure random bytes. `std.hash` is for tables, not security:
`hash_of(value)`, `Fnv1a`, `DefaultHasher`.

```hd
use std.crypto.sha256.{Sha256, sha256}

tests:
    it("hashes"):
        assert_equal(sha256("abc".to_utf8()).to_hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        tag := Sha256::hmac("key".to_utf8()).update("msg".to_utf8()).finish()
        assert_equal(tag.to_hex().len(), 64)
```

## Validation

`std.validate` checks values with rules written as field facts. Derive `Validate`, put `@range(1, 9)`,
`@email()`, `@length(1, 40)` on fields, and `value.validated(10)` returns `Ok`, or a `ValidationError` whose
`issues()` list each `Violation` with its path (10 is the most errors to collect). Rules: `range`, `at_least`, `max`, `length`, `min_length`, `max_length`, `pattern`, `email`, `url`, `uuid`,
`one_of`. See [design/derive.md](design/derive.md) and [scenarios/configuration.md](scenarios/configuration.md).

## Testing, errors, contracts and the rest

- `std.testing`: `it_each`, `it_prop` with `Arbitrary`, `snapshot`, `snapshot_file`, `temp_dir`, `hd_run!`.
  Fakes (`MemoryConsole`, `ScriptedInput`, `MemoryFs`, `ScriptedEnv`, `ManualClock`, `ScriptedProcess`,
  `ScriptedHttp`, `ScriptedNet`) are in [design/testing.md](design/testing.md).
- `std.error`: the `Error` trait, `message(text)` for an ad-hoc error, `report_of(&error)`; `std.option` and
  `std.result`: `unwrap`, `expect`, `unwrap_or`, `map`, `and_then`, `ok`, `map_err` ([design/errors.md](design/errors.md)).
- `std.convert`: `From` and `TryFrom`. `std.ops`: operator traits, `Default`, `Fn`, `defer`.
- `std.contract`: `require` and `ensure` ([design/contracts.md](design/contracts.md)).
- `std.meta`: `Inspectable`, `Structure`, `@inline` and `@cold`; for writing derives ([design/derive.md](design/derive.md)).
- `std.arena`: `Arena::new()`, `arena.alloc(v)` gives a `&mut T`, and `arena.reset()` drops everything at once.
  Use it for many short-lived values of one phase.

```hd
use std.arena.Arena

tests:
    it("allocates in an arena"):
        let mut arena = Arena::new()
        n := arena.alloc(5)
        *n += 1
        assert_equal(*n, 6)
        arena.reset()
```
