# Text

A `string` is immutable UTF-8 text. Its methods need no import. Every
position a method takes or returns is a **byte** offset.
`let text: string = chars.collect()` joins an iterator of `char` values as UTF-8.

## Methods

| Call | Result |
| --- | --- |
| `s.len()` | byte count, `usize` |
| `s.is_empty()` | `bool` |
| `s.contains(t)`, `s.starts_with(t)`, `s.ends_with(t)` | `bool` |
| `s.find(t)`, `s.rfind(t)` | first or last byte offset, `usize?` |
| `s.count(t)` | non-overlapping matches, `usize` |
| `s.slice(start, end)` | the bytes from `start` up to `end`, `string` |
| `s.trim()`, `s.trim_start()`, `s.trim_end()` | `string` |
| `s.lower()`, `s.upper()` | `string` |
| `s.replace(old, new)` | `string`, every match replaced |
| `s.repeat(n)` | `string` |
| `s.pad_start(width)`, `s.pad_end(width)` | `string`; a second argument gives the fill `char` |
| `s.strip_prefix(t)`, `s.strip_suffix(t)` | the rest, `string?` |
| `s.split(sep)` | `List[string]`, keeping empty pieces |
| `s.splitn(sep, n)` | at most `n` pieces |
| `s.split_once(sep)` | `(string, string)?` around the first match |
| `s.split_whitespace()` | `List[string]` of the words |
| `s.lines()` | `List[string]`, without line endings |
| `s.chars()` | an iterator of `char` |
| `s.char_indices()` | an iterator of `(usize, char)` |
| `s.bytes()` | an iterator of `u8` |
| `s.parse::[T]()` | `Result[T, ...]` for a number, `bool`, `char` and more |
| `list.join(sep)` | the `Display` text of a `List[T]` joined into one `string` |
| `a + b` | two strings joined |

```hd
line := "  name = Ada Lovelace  "
let .Some((key, value)) = line.trim().split_once(" = ") else: panic("no '='")
words := value.split_whitespace()
initials := [for word in words => word.slice(0, 1)].join(".")
println("$key|${value.upper()}|${words.len()}|$initials|${debug(value.find("Love"))}")
println("${"7".pad_start(3, '0')} ${"a,b,,c".split(",").len()} ${"ab".repeat(2)} ${debug("x.txt".strip_suffix(".txt"))}")
```

```output
name|ADA LOVELACE|2|A.L|Option.Some(4)
007 4 abab Option.Some("x")
```

## Bytes and characters

`len()` and `slice` count bytes. A slice that cuts a character in half
panics. For text beyond ASCII, walk it with `chars()` or `char_indices()`.

```hd
word := "héllo"
let letters = 0
for c in word.chars():
    if c.is_alphabetic():
        letters += 1
println("${word.len()} bytes, ${word.chars().count()} chars, $letters letters")
for (offset, c) in "aé".char_indices():
    println("$offset $c")
```

```output
6 bytes, 5 chars, 5 letters
0 a
1 é
```

`char` methods: `is_alphabetic()`, `is_alphanumeric()`, `is_uppercase()`, `is_ascii_digit()`,
`is_whitespace()`, `to_digit(radix)`, `to_u32()`.

## Parsing

`parse::[T]()` returns a `Result`. Handle it with `match`, `?`,
`unwrap_or`, or `expect`.

```hd
fn port(text: string) -> i32:
    text.trim().parse::[i32]().unwrap_or(8080)

println("${port(" 9000 ")} ${port("many")} ${"2.5".parse::[f64]().expect("a number")} ${"true".parse::[bool]().is_ok()}")
```

```output
9000 8080 2.5 true
```

The error of a number parse is `std.num.ParseNumberError`.

## Building text

Use `join` for a list with a separator, and `StringBuilder` for a loop
that appends.

```hd
use std.text.StringBuilder

fn table(rows: List[(string, i32)]) -> string:
    out := StringBuilder::new()
    for row in rows:
        out.push(row._0.pad_end(8))
        out.push(row._1.to_string().pad_start(5))
        out.push("\n")
    out.build()

println(table([("tea", 250), ("cake", 1200)]).trim_end())
println(["a", "b", "c"].join(", "))
```

```output
tea       250
cake     1200
a, b, c
```

A number becomes text with `n.to_string()` or `"$n"`.

## Regex

`std.regex` compiles a pattern once and runs in linear time. Write the
pattern as a raw string, `r"..."`, so `\d` needs no second backslash.

To pull several fields out of a line, name the groups after the fields of
a `@derive(Deserialize)` struct and call `decode::[T]`: it matches, then
parses each group as its field's type.

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

`decode` returns `RegexError.NoMatch` when the line does not match, and
`WrongType` naming the group that does not parse. A `T?` field is `.None`
when its group took no part. For one group, `pattern.captures(line)`
gives the groups: `get(1)` by number, `name("status")` by name, each a
`Match?` with `.text`. `is_match`, `find`, `find_all`, `replace_all` and
`split` cover the rest.
