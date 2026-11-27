# How To: Common Operations

**Scenario.** The everyday operations of a parser, a renderer or a data
tool, each as a short tested block, followed by the clue that trips
agents up. For every item of a module, see the generated
[std API pages](../std/api/README.md).

## Strings: bytes, characters, slices

```hd
tests:
    it("strings"):
        s := "héllo, world"
        assert_equal(s.len(), 13)                       # bytes, not characters
        assert_equal(s.chars().count(), 12)             # characters
        assert_equal(s.slice(0, 1), "h")                # byte offsets
        assert_equal(s.find(","), .Some(6))             # a byte offset too
        assert_equal(s.strip_prefix("hé"), .Some("llo, world"))
        assert_equal(s.split_once(", "), .Some(("héllo", "world")))
        assert_equal("ab".repeat(3), "ababab")
        assert_equal("7".pad_start(3, '0'), "007")
        assert_equal(" x ".trim(), "x")
        assert_equal("Tea".lower(), "tea")
        let bytes: List[u8] = "hi".bytes().collect()
        assert_equal(bytes, [104, 105])
        assert_equal(string::from_utf8([104, 105]), .Ok("hi"))
        for (i, c) in "ab".char_indices():              # (byte offset, char)
            assert(c == 'a' || i == 1)
```

- `len()`, `slice`, `find` and `char_indices` count **bytes**. A slice
  that cuts a character in two panics; take offsets from `find` or
  `char_indices`, never from counting `chars()`.
- `slice` makes a new string. In a loop over a long text, keep an index
  instead of re-slicing (see [the scanner](#a-scanner-over-bytes)).
- A `string` is always valid UTF-8. `string::from_utf8(bytes)` checks it
  and returns a `Result`.

## Characters and their codes

```hd
use std.convert.TryFrom

tests:
    it("chars"):
        assert_equal('A'.to_u32(), 65)
        assert_equal(char::try_from(97u32), .Ok('a'))
        assert_equal('7'.to_digit(10), .Some(7))
        assert('x'.is_alphabetic())
        assert('5'.is_ascii_digit())
```

- `char::try_from` and `u8::try_from` need `use std.convert.TryFrom`.
  Without it the error is `unknown-method: no method try_from`.

## Building text

```hd
use std.text.StringBuilder

tests:
    it("builds text"):
        let mut out = StringBuilder::new()
        for n in 0..3:
            out.push("$n")
            out.push_char(',')
        assert_equal(out.build(), "0,1,2,")
```

- Build long output with a `StringBuilder` and print it once. `s = s + x`
  in a loop copies `s` each time, which is quadratic.

## Numbers: parsing, converting, formatting

```hd
use std.convert.TryFrom

tests:
    it("numbers"):
        assert_equal("42".parse::[i64](), .Ok(42))
        assert_equal(u32::parse_radix("ff", 16), .Ok(255))
        let n: i64 = 300
        assert(u8::try_from(n).is_err())                 # narrowing can fail
        assert_equal(i64(7u8), 7)                        # widening cannot
        assert_equal(3.14159.to_fixed(2), "3.14")
        assert_equal(i32(7) / 2, 3)                      # integer division truncates
        assert_equal(i32(-7) % 3, -1)                    # the sign follows the dividend
        assert_equal(i32(2147483647).checked_add(1), .None)
        assert((0.1 + 0.2).to_bits() != 0.3.to_bits())
```

- Integers never convert implicitly. Widen with `i64(x)`, and narrow with
  `T::try_from(x)`, which returns a `Result`.
- `parse_radix` is called on the integer type: `u32::parse_radix(text, 16)`.
- Arithmetic overflow panics. Use `checked_add`, `wrapping_add` or
  `saturating_add` when overflow is expected.
- A `let` with a type annotation is `let n: i64 = 300`. `n: i64 := 300` is
  a syntax error.

## Lists

```hd
tests:
    it("lists"):
        let mut xs = [3, 1, 2]
        xs.sort()
        assert_equal(xs, [1, 2, 3])
        xs.sort_by(fn(a, b): b.cmp(a))                  # descending
        assert_equal(xs, [3, 2, 1])
        assert_equal(xs.slice(0, 2), [3, 2])
        assert_equal(xs.get(9), .None)                   # never panics
        assert_equal(xs.index_of(2), .Some(1))
        assert_equal(xs.last(), .Some(1))
        evens := [for x in 0..10 if x % 2 == 0 => x]
        assert_equal(evens.len(), 5)
        assert_equal([1, 2].zip(["a", "b"]), [(1, "a"), (2, "b")])
```

- A comprehension puts its clauses first: `[for x in xs if p(x) => f(x)]`.
- `xs[i]` panics past the end, and `xs.get(i)` returns `.None`.
- A list is a value: passing it to a function or assigning it copies it.
  Pass `&xs` to read it, or `&mut xs` to change it in place.

## Maps

```hd
tests:
    it("maps"):
        let mut ages: Map[string, i32] = {}
        ages["ada"] = 36
        *ages.get_or_insert("bob", 0) += 1               # count or update
        assert_equal(ages["bob"], 1)
        assert(ages.contains_key("ada"))
        let names: List[string] = ages.keys().collect()
        assert_equal(names, ["ada", "bob"])              # insertion order
```

- `keys()`, `values()` and `entries()` return iterators. `collect()`
  them into a typed `let` to get a list.
- Iteration follows insertion order. Sort the keys when the output must
  be sorted.

## A tree

```hd
enum Expr:
    Num(i64)
    Add(*Expr, *Expr)
    Mul(*Expr, *Expr)

fn eval(e: &Expr) -> i64:
    match e:
        .Num(n) => n
        .Add(l, r) => eval(l) + eval(r)
        .Mul(l, r) => eval(l) * eval(r)

tests:
    it("evaluates a tree"):
        e := Expr.Add(box(Expr.Num(2)), box(Expr.Mul(box(Expr.Num(3)), box(Expr.Num(4)))))
        assert_equal(eval(&e), 14)
```

- A type can't hold itself by value. A recursive variant holds its
  children through `*Expr`, made with `box(...)`.
- A pattern binds a copy of a small value (`.Num(n) => n`). `*n` is only
  for references.

## A scanner over bytes

```hd
struct Scanner:
    bytes: List[u8]
    at: usize

impl Scanner:
    fn new(text: string) -> Scanner:
        Scanner { bytes: text.to_utf8(), at: 0 }

    fn peek(&self) -> u8?:
        self.bytes.get(self.at)

    fn bump(&mut self) -> void:
        self.at += 1

    fn number(&mut self) -> i64:
        let n: i64 = 0
        while true:
            match self.peek():
                .Some(b) if b >= 48 && b <= 57 =>       # '0'..'9': hd has no byte literal
                    n = n * 10 + i64(b - 48)
                    self.bump()
                _ => break
        n

tests:
    it("scans a number"):
        let mut s = Scanner::new("123+4")
        assert_equal(s.number(), 123)
        assert_equal(s.peek(), .Some(43))
```

- A lexer or parser keeps its input and position in a struct, and its
  methods take `&mut self`. Index into the bytes; don't re-slice the
  string per token.
- hd has no byte literal: compare bytes with numbers, or work in `char`s
  with `chars()` when the input isn't ASCII.
