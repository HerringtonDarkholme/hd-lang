# Json

Status: standard library specification draft.

This chapter defines the untyped part of `std.json`, which `lib/std` writes
in ordinary hd over the language tier:

- the value type `Json`, and `Number`, which keeps 64-bit integers exact;
- `parse`, a strict [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) parser;
- `JsonError`, the error that `parse` returns;
- the compact `Display` text of a `Json`, and `pretty`.

Nothing in it is language tier. The typed part, the traits `ToJson` and
`FromJson` with `encode` and `decode`, is not specified yet.

1. r[std-json.import] `std.json` declares `Json`, `Number`, `JsonError`, `parse`, and `pretty`. None is a prelude name; code imports them, as in `use std.json.parse`.
2. r[std-json.no-panic] None of these functions panics.

## Json Values

A `Json` is one of six kinds:

```text
pub enum Json:
    Null
    Bool(value: bool)
    Number(value: Number)
    Text(value: string)
    Array(items: List[Json])
    Object(fields: Map[string, Json])
```

1. r[std-json.value.decl] `std.json` declares the enum `Json` with exactly the variants above.
2. r[std-json.value.object-order] An `Object` keeps its keys in [insertion order](../lang/04-type-system.md#r-types.map.order), as every `Map` does.
3. r[std-json.value.eq] `Json` implements `Eq`. Two values are equal when they are the same variant with equal contents.
4. r[std-json.value.eq.array] Arrays are equal when they have equal items in the same order.
5. r[std-json.value.eq.object] Objects are equal as maps are, so key order does not matter ([`types.map.semantics`](../lang/04-type-system.md#r-types.map.semantics)).
6. r[std-json.value.debug] `Json`, `Number`, and `JsonError` implement `Debug`, as [`std-format.debug.std-types`](format.md#r-std-format.debug.std-types) requires.

The accessors read one kind and give `.None` for any other:

| Rule | Method | Result |
| --- | --- | --- |
| r[std-json.value.accessors.get] `get` | `pub fn get(self, key: string) -> Json?` | the value of `key` when `self` is an `Object` with that key |
| r[std-json.value.accessors.at] `at` | `pub fn at(self, index: i32) -> Json?` | the item at `index` when `self` is an `Array` and `index` is from 0 up to its length |
| r[std-json.value.accessors.text] `as_text` | `pub fn as_text(self) -> string?` | the string of a `Text` |
| r[std-json.value.accessors.bool] `as_bool` | `pub fn as_bool(self) -> bool?` | the value of a `Bool` |
| r[std-json.value.accessors.number] `as_number` | `pub fn as_number(self) -> Number?` | the `Number` of a `Number` |
| r[std-json.value.accessors.null] `is_null` | `pub fn is_null(self) -> bool` | `true` exactly for `Null` |

1. r[std-json.value.accessors] These methods are declared on `Json` in `std.json`. A negative `index` gives `.None`.

> **Note.** The accessors take no path and no default. Chain them with
> `and_then` ([Option](option.md)).

## Numbers

A `Number` is a JSON number whose representation is private. It follows
Rust's `serde_json::Number`: integers in the `i64` and `u64` ranges are
exact, and every other number is an `f64`.

```text
use std.json.Number

fn big() -> Number:
    Number::from_u64(18446744073709551615)

fn half() -> Number?:
    Number::from_f64(0.5)
```

1. r[std-json.number.decl] `std.json` declares the type `Number`, with no public member.
2. r[std-json.number.repr] A `Number` holds an integer below zero, an integer from zero up, or a finite `f64`. One integer has one representation, whichever constructor made it.

| Rule | Function | Result |
| --- | --- | --- |
| r[std-json.number.constructors.i64] `from_i64` | `pub fn from_i64(value: i64) -> Number` | the integer `value` |
| r[std-json.number.constructors.u64] `from_u64` | `pub fn from_u64(value: u64) -> Number` | the integer `value` |
| r[std-json.number.constructors.f64] `from_f64` | `pub fn from_f64(value: f64) -> Number?` | the float `value`, or `.None` for a NaN or an infinity |

1. r[std-json.number.constructors] These three are static functions of `Number`, called as `Number::from_i64(value)`.

| Rule | Method | Result |
| --- | --- | --- |
| r[std-json.number.accessors.i64] `as_i64` | `pub fn as_i64(self) -> i64?` | the integer when it is in the `i64` range, and `.None` for a float |
| r[std-json.number.accessors.u64] `as_u64` | `pub fn as_u64(self) -> u64?` | the integer when it is from zero up, and `.None` for a negative integer or a float |
| r[std-json.number.accessors.f64] `as_f64` | `pub fn as_f64(self) -> f64?` | `.Some` of the float, or of the integer converted to the nearest `f64` |

1. r[std-json.number.accessors] These three are methods of `Number`. `as_f64` gives `.Some` for every `Number`, so no `.None` result means a number is out of range.

### Number Equality

1. r[std-json.number.eq] `Number` implements `Eq`, with the rule of `serde_json`: two numbers are equal when both are integers with the same value, or both are floats that are equal as `f64` values.
2. r[std-json.number.eq.mixed] An integer is never equal to a float, so `1` and `1.0` differ.
3. r[std-json.number.eq.zero] The floats `0.0` and `-0.0` are equal, as `f64` values are.

### Number Text

1. r[std-json.number.display] `Number` implements `Display`. An integer prints in base ten, as [`types.display.int`](../lang/04-type-system.md#r-types.display.int) gives, and a float prints as `f64` does ([`types.display.float`](../lang/04-type-system.md#r-types.display.float)).

> **Note.** The text of a finite float is always valid JSON. It has a
> decimal point or an exponent, as `1.0` and `1e+21`, and negative zero is
> `-0.0` ([`types.display.fixed`](../lang/04-type-system.md#r-types.display.fixed)).

> **Why.** `serde_json` keeps three representations for the same reason:
> a `u64` above `i64::MAX` and an `i64` below zero must survive a round
> trip that an `f64` would round. A JSON number is a decimal text, so a
> library that read every number as an `f64` would lose the low bits of an
> identifier.

## Parsing

`parse` reads one JSON text:

```text
use std.json.{parse, Json, JsonError}

fn title(text: string) -> string:
    match parse(text):
        .Ok(value) => value.get("title").and_then(fn(found): found.as_text()).unwrap_or("")
        .Err(error) => "$error"
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-json.parse] `parse` | `pub fn parse(text: string) -> Result[Json, JsonError]` | `.Ok` of the value that `text` writes, or `.Err` of the first error |

### Grammar

1. r[std-json.parse.grammar] `parse` accepts exactly the texts that the grammar of RFC 8259 gives for a JSON text, with any value at the top.
2. r[std-json.parse.whitespace] Whitespace is a space, a tab, a line feed, or a carriage return. It may stand before and after the value, and between any two tokens.
3. r[std-json.parse.strict] The grammar has no extension: no comment, no trailing comma, no single-quoted string, no `NaN` or `Infinity`, and no byte order mark.
4. r[std-json.parse.end] Only whitespace may follow the value. The first other character is an `UnexpectedCharacter` error.
5. r[std-json.parse.depth] An array or object nested inside 128 others is an error, and one nested inside 127 is not. The error is `NestingTooDeep`, at its opening bracket.
6. r[std-json.parse.literals] The words `true`, `false`, and `null` are lowercase only.

### Objects

1. r[std-json.parse.object.order] The keys of a parsed object are in the order of their first occurrence in the text.
2. r[std-json.parse.object.duplicate] When a key occurs more than once, the object has one entry for it. Its value is the last one, and its position is the first one's.
3. r[std-json.parse.object.key] A key is a string, and its escapes are decoded as a string's are.

### Strings

1. r[std-json.parse.string.escapes] An escape is a backslash and one of the following, and any other letter after a backslash is an `InvalidEscape` error.

| Escape | Value |
| --- | --- |
| `\"`, `\\`, `\/` | the character after the backslash |
| `\b`, `\f`, `\n`, `\r`, `\t` | U+0008, U+000C, U+000A, U+000D, U+0009 |
| `\u` and four hex digits | the UTF-16 code unit of those digits, in either case |

1. r[std-json.parse.string.hex] A `\u` not followed by four hex digits is an `InvalidEscape` error at its backslash. When the text ends first, the error is `UnexpectedEnd`.
2. r[std-json.parse.string.surrogates] A high surrogate escape, from `\ud800` to `\udbff`, must be followed at once by a low surrogate escape, from `\udc00` to `\udfff`. The pair is one scalar value.
3. r[std-json.parse.string.lone-surrogate] A surrogate escape with no partner is a `LoneSurrogate` error at the backslash of the first escape of the pair.
4. r[std-json.parse.string.control] A raw character below U+0020 inside a string is a `ControlCharacter` error. A raw DEL (U+007F) and every other scalar value stand for themselves.
5. r[std-json.parse.string.nul] `\u0000` is valid, and the string then holds U+0000.

### Number Syntax

1. r[std-json.parse.number.grammar] A number is `-`, then `0` or a nonzero digit and digits, then an optional fraction, then an optional exponent. A character that breaks this is an `UnexpectedCharacter` error, or `UnexpectedEnd` at the end of the text.
2. r[std-json.parse.number.integer] An integer in the `i64` range or in the `u64` range is exact, as `Number.as_i64` or `Number.as_u64` reads it.
3. r[std-json.parse.number.negative-zero] `-0` is the float `-0.0`, as in `serde_json`, and `0` is the integer zero.
4. r[std-json.parse.number.float] A number with a fraction or an exponent, or an integer outside both ranges, is the float that [`parse_f64`](num.md#r-std-num.parse-f64.decimal-value) gives for its text.
5. r[std-json.parse.number.out-of-range] A float past the finite `f64` range, which `parse_f64` gives as an infinity, is a `NumberOutOfRange` error at the number's first character, once its grammar is read to the end.

| Text | `parse` gives |
| --- | --- |
| `"1.5"` | `.Ok` of the float `1.5` |
| `"18446744073709551616"` | `.Ok` of the float 2^64, one past the `u64` range |
| `"1e-400"` | `.Ok` of the float `0.0` |
| `"1e400"` | `.Err(NumberOutOfRange(0))` |

> **Why.** `serde_json` reads numbers the same way. An integer in range is
> exact, and every other number is the nearest `f64`. A number past the
> `f64` range is an error, since a `Number` holds no infinity
> ([`std-json.number.repr`](#r-std-json.number.repr)).

### Error Order

1. r[std-json.parse.error-order] `parse` reads the text from the left. Its error is the first position at which no continuation is valid, with the kind that the position gives.

| Text | `parse` gives |
| --- | --- |
| `""` | `.Err(UnexpectedEnd(0))` |
| `"[1,]"` | `.Err(UnexpectedCharacter(3))` |
| `"01"` | `.Err(UnexpectedCharacter(1))` |
| `"\"\\ud800\""` | `.Err(LoneSurrogate(1))` |
| `"1e400"` | `.Err(NumberOutOfRange(0))` |
| `"1.x"` | `.Err(UnexpectedCharacter(2))` |

## JSON Errors

A failed parse reports one of seven errors, each at a position:

```text
pub enum JsonError:
    UnexpectedEnd(position: i32)
    UnexpectedCharacter(position: i32)
    InvalidEscape(position: i32)
    LoneSurrogate(position: i32)
    ControlCharacter(position: i32)
    NumberOutOfRange(position: i32)
    NestingTooDeep(position: i32)
```

1. r[std-json.error.enum] `std.json` declares the enum `JsonError` with the variants above, each with one field `position: i32`.
2. r[std-json.error.position] A `position` is an index into the text, counted in characters from 0.
3. r[std-json.error.traits] `JsonError` implements `Eq`, `Debug`, and `Display`.

| Rule | Variant | Position |
| --- | --- | --- |
| r[std-json.error.kind.end] `UnexpectedEnd` | the text stops before the value is complete | the length of the text |
| r[std-json.error.kind.character] `UnexpectedCharacter` | a character that the grammar does not allow there | that character |
| r[std-json.error.kind.escape] `InvalidEscape` | a backslash escape that is not in the table, or a `\u` without four hex digits | the backslash |
| r[std-json.error.kind.surrogate] `LoneSurrogate` | a surrogate escape with no partner | the backslash of the first escape |
| r[std-json.error.kind.control] `ControlCharacter` | a raw character below U+0020 in a string | that character |
| r[std-json.error.kind.range] `NumberOutOfRange` | a number past the finite `f64` range | its first character |
| r[std-json.error.kind.depth] `NestingTooDeep` | an array or object past 128 levels | its opening bracket |

> **Note.** The text is valid UTF-8, so a position in characters is not a
> byte offset when a non-ASCII character comes before it.

## Writing

A `Json` writes as JSON text in two layouts, compact and pretty:

```text
use std.json.{parse, pretty, Json}

fn show(value: Json) -> string:
    "${value}"   # {"a":[1,2]}

fn show_pretty(value: Json) -> string:
    pretty(value)
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-json.pretty] `pretty` | `pub fn pretty(value: Json) -> string` | the pretty text of `value` |

### Compact Text

1. r[std-json.display.compact] `Json` implements `Display`. Its text has no whitespace outside strings: `[1,2]` and `{"a":1}`.
2. r[std-json.display.kinds] `Null` prints as `null`, a `Bool` as `true` or `false`, and a `Number` as [`std-json.number.display`](#r-std-json.number.display) gives.
3. r[std-json.display.containers] An `Array` prints its items in order, and an `Object` prints its entries in insertion order as `"key":value`, each list separated by `,`.
4. r[std-json.display.text] A `Text` and each key print in quotes. A quote, a backslash, and each character below U+0020 are escaped, and every other scalar value prints as itself.
5. r[std-json.display.escapes] The escapes are `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, and `\t`, and `\u00` with two lowercase hex digits for the other controls. `/` is not escaped.

> **Note.** The compact text of a value that `parse` read parses back to an
> equal value.

### Pretty Text

1. r[std-json.pretty.layout] `pretty` writes the compact text's tokens with each item of an array and each entry of an object on its own line, indented by two spaces for each level of nesting.
2. r[std-json.pretty.separator] An entry is `"key": value`, with one space after the colon. A comma follows every item or entry but the last, with no space before it.
3. r[std-json.pretty.empty] An empty array prints as `[]` and an empty object as `{}`, on one line.
4. r[std-json.pretty.scalar] A value that is not an array or an object prints as its compact text, with no line break.
5. r[std-json.pretty.no-trailing] The text ends with its last bracket, brace, or scalar: no trailing space, no final line break.

| `Json` | `pretty` text |
| --- | --- |
| `[1,2]` | `"[\n  1,\n  2\n]"` |
| `{"a":[]}` | `"{\n  \"a\": []\n}"` |

See also: [Encoding](encoding.md#decode-errors), [Num](num.md#integer-parsing), [Collections](collections.md#map-methods), [Numeric Display](../lang/04-type-system.md#numeric-display), [Map Lookup And Order](../lang/04-type-system.md#lookup-and-order).
