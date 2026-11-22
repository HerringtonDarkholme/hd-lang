# Basics: Files, Bindings, Numbers, Strings

## Two kinds of file

A file is a **script** when it has top-level statements. They run in order,
and the script may use every host capability (printing, files, the clock).

```hd
# `#` starts a comment.
name := "Ada"
println("hello, $name")
```

```output
hello, Ada
```

A file is a **program** when it has `pub fn main`. `main` lists what it
uses after `$`; `println` needs `$ Console`
([requirements.md](requirements.md)).

```hd
pub fn main() -> void $ Console:
    println("hello")
```

Use `main` when the file has a `tests:` block, since a test run runs no
script body, and in every package (`src/main.hd`).

## Blocks

A header ends in `:` and opens an indented body (four spaces), or a
same-line body. There are no braces. `pass` is the empty body.

```hd
fn sign(n: i32) -> i32:
    if n > 0: 1 else if n < 0: -1 else: 0

fn noop() -> void:
    pass
```

An expression that continues on the next line needs parentheses, unless
the next line starts with `.` or `|>`:

```hd
fn ready(a: bool, b: bool, text: string) -> bool:
    ok := (a
        && b)
    shout := text
        .trim()
        .upper()
    ok && shout.len() > 0
```

## Bindings

| Form | Meaning |
| --- | --- |
| `name := expr` | a name that is never reassigned; no type annotation |
| `let name = expr` | a name that can be reassigned |
| `let name: T = expr` | the same, with a type; an annotation always needs `let` |
| `let mut name = expr` | a list, map or struct that is changed in place |
| `let (a, b) = pair` | destructuring; always `let` |
| `_ := expr` | evaluate and discard a value that must be used |

```hd
LIMIT := 3                      # top level: a private constant

fn demo() -> usize:
    let count = 0
    count = count + LIMIT       # reassigning needs `let`
    let mut seen: List[string] = []
    seen.push("a")              # changing in place needs `let mut`
    let (low, high) = (1, 9)
    _ := seen.pop()             # `pop` returns an optional, which must be used
    count + seen.len() + low + high
```

A number or string needs only `let` to change, because changing it is a
reassignment. `let mut` on one is accepted, and `hd lint` reports it as
`redundant-let-mut`. Top-level bindings are private: there is no `pub let`
and no `pub NAME :=`; export a function instead.

## Numbers

Types: `i8` `i16` `i32` `i64`, `u8` `u16` `u32` `u64`, `usize` (lengths,
indices, byte offsets), `f32`, `f64`. There is no `isize` and no `int`.

- An integer literal takes its type from how the body uses it. With no use
  that decides, it is `usize`, or `i32` when written with a sign, as `+5`.
  A float literal is `f64`.
- A suffix fixes the type: `1u32`, `255u8`, `-3i64`, `1.5f32`. A hex
  literal takes no suffix: write `let mask: u64 = 0xFF`.
- Conversions are calls: `i32(n)`, `usize(n)`, `f64(n)`, `i64(n)`. Nothing
  converts implicitly, and signed and unsigned integers do not mix.
- `/` on integers truncates, `%` is the remainder, `**` is a power.
- Integer overflow panics (with `--release` it wraps instead), and so does
  division by zero.

```hd
fn average(scores: List[i32]) -> f64:
    let sum: i64 = 0
    for score in scores:
        sum += i64(score)
    f64(sum) / f64(scores.len())

fn last_index(scores: List[i32]) -> i32:
    i32(scores.len()) - 1       # `len()` is usize; convert before signed arithmetic

println("${average([+90, 85])} ${last_index([+1, 2, 3])} ${7 / 2} ${7 % 2} ${2 ** 10} ${0xFF}")
```

```output
87.5 2 3 1 1024 255
```

## Strings and characters

A `string` is immutable UTF-8 text. `"$name"` and `"${expression}"`
interpolate; `"\$"` is a dollar sign. A `char` is written `'a'`.

```hd
use std.text.r

name := "Ada"
initial := 'A'
count := 3
line := "$name has $count items; first letter $initial; cost \$${count * 2}"
pattern := r"\d+ items"         # a raw string: backslashes stay as written
println("$line / $pattern / ${line.len()} bytes")
```

```output
Ada has 3 items; first letter A; cost $6 / \d+ items / 40 bytes
```

- Only a `Display` type interpolates: numbers, `bool`, `char`, `string`, and
  types that implement `Display`. For an optional, a list, a map or a
  struct, write `${debug(x)}`, or unwrap the optional first.
- `len()` counts bytes. `s.chars().count()` counts characters.
- `+` joins two strings. To build text in a loop, use `parts.join(",")` or
  a `StringBuilder` ([text.md](../std/text.md)).
- `dbg(x)` prints a value with its source location to standard error. It
  returns `void`, so it is a statement, not a wrapper.
