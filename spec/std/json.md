# Json

Status: standard library specification draft.

This chapter defines `std.json`, which `lib/std` writes in ordinary hd
over the language tier:

- the value type `Json`, and `Number`, which keeps 64-bit integers exact;
- `parse`, a strict [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) parser;
- `JsonError`, the error that `parse` and `decode` return;
- the compact `Display` text of a `Json`, and its `pretty` method;
- the typed part: JSON as a format over the `std.serde` consent, with
  `to_json`, `from_json`, `encode`, and `decode`.

Nothing in it is language tier.

1. r[std-json.import.core] `std.json` declares `Json`, `Number`, `JsonError`, and `parse`. None is a prelude name; code imports them, as in `use std.json.parse`.
2. r[std-json.import.format] `std.json` also declares `to_json`, `from_json`, `encode`, and `decode`. None is a prelude name either.
3. r[std-json.no-panic] None of these functions panics.

```text
use std.json.{Json, JsonError, parse}

fn read(text: string) -> Result[Json, JsonError]:
    parse(text)   # a bad text is an .Err: no call here panics
```

## Json Values

A `Json` is one of six kinds:

```text
pub enum Json:
    Null
    Bool(value: bool)
    Number(value: Number)
    Text(value: string)
    Array(items: List[Json])
    Object(fields: JsonObject)
```

1. r[std-json.value.decl] `std.json` declares the enum `Json` with exactly the variants above.
2. r[std-json.value.object-type] An `Object` holds a [`JsonObject`](#json-objects), which keeps its members in insertion order. It is not a `Map`, whose [iteration order](../lang/04-type-system.md#r-types.map.order.unspecified) is unspecified.
3. r[std-json.value.eq] `Json` implements `Eq`. Two values are equal when they are the same variant with equal contents.
4. r[std-json.value.eq.array] Arrays are equal when they have equal items in the same order.
5. r[std-json.value.eq.object] Objects are equal when they have the same keys with equal values. Their order does not matter.
6. r[std-json.value.debug] `Json`, `Number`, and `JsonError` implement `Debug`, as [`std-format.debug.std-types`](format.md#r-std-format.debug.std-types) requires.

The accessors read one kind and give `.None` for any other:

| Rule | Method | Result |
| --- | --- | --- |
| r[std-json.value.accessors.get] `get` | `pub fn get(self, key: string) -> Json?` | the value of `key` when `self` is an `Object` with that key |
| r[std-json.value.accessors.at-index] `at` | `pub fn at(self, index: usize) -> Json?` | the item at `index` when `self` is an `Array` and `index` is from 0 up to its length |
| r[std-json.value.accessors.text] `as_text` | `pub fn as_text(self) -> string?` | the string of a `Text` |
| r[std-json.value.accessors.bool] `as_bool` | `pub fn as_bool(self) -> bool?` | the value of a `Bool` |
| r[std-json.value.accessors.number] `as_number` | `pub fn as_number(self) -> Number?` | the `Number` of a `Number` |
| r[std-json.value.accessors.null] `is_null` | `pub fn is_null(self) -> bool` | `true` exactly for `Null` |

1. r[std-json.value.accessors] These methods are declared on `Json` in `std.json`. A negative `index` gives `.None`.

> **Note.** The accessors take no path and no default. Chain them with
> `and_then` ([Option](option.md)).

## Json Objects

`JsonObject` is the type of an `Object`'s members. Insertion order is part
of its contract, so a config file parsed and written again keeps its keys
where they were.

| Rule | Method | Result |
| --- | --- | --- |
| r[std-json.object.new] `new` | `pub fn new() -> mut JsonObject` | an object with no members |
| r[std-json.object.len] `len` | `pub fn len(self) -> usize` | the number of members; `is_empty` is `len() == 0` |
| r[std-json.object.get] `get` | `pub fn get(self, key: string) -> Json?` | the value of `key`, or `.None`; `contains_key` is `true` for a key that has a member |
| r[std-json.object.set] `set` | `pub fn set(mut self, key: string, value: Json) -> void` | a new key becomes the last member, and an existing key keeps its place with the new value |
| r[std-json.object.keys] `keys` | `pub fn keys(self) -> List[string]` | the keys, in insertion order |

1. r[std-json.object.decl] `std.json` declares `JsonObject` with the methods above, and `JsonObject` implements `Iterable[(string, Json)]`, `Eq`, and `Debug`.
2. r[std-json.object.order] `keys`, iteration, and `Debug` give the members in insertion order.
3. r[std-json.object.eq] Two `JsonObject`s are equal when they have the same keys with equal values, in any order.

```text
use std.json.{Json, JsonObject}

fn build() -> Json:
    let fields: mut JsonObject = JsonObject::new()
    fields.set("name", Json.Text("hd"))
    fields.set("id", Json.Null)
    fields.set("name", Json.Bool(true))   # keeps its place: keys() is ["name", "id"]
    Json.Object(fields)
```

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

1. r[std-json.number.eq] `Number` implements `Eq`, with the rule of `serde_json`. Two numbers are equal when both are integers with the same value, or both are floats that are equal as `f64` values.
2. r[std-json.number.eq.mixed] An integer is never equal to a float, so `1` and `1.0` differ.
3. r[std-json.number.eq.zero] The floats `0.0` and `-0.0` are equal, as `f64` values are.

```text
use std.json.Number

fn is_integer_two(n: Number) -> bool:
    n == Number::from_i64(2)   # false when n is the float 2.0
```

### Number Text

1. r[std-json.number.display] `Number` implements `Display`. An integer prints in base ten, as [`types.display.int`](../lang/04-type-system.md#r-types.display.int) gives, and a float prints as `f64` does ([`types.display.float`](../lang/04-type-system.md#r-types.display.float)).

```text
use std.json.Number

fn show(n: Number) -> string:
    "$n"   # an integer prints in base ten, a float as an f64
```

> **Note.** The text of a finite float is always valid JSON. It has a
> decimal point or an exponent, as `1.0` and `1e+21`, and negative zero is
> `-0.0` ([`types.display.fixed`](../lang/04-type-system.md#r-types.display.fixed)).

> **Why.** `serde_json` keeps three representations for the same reason.
> A `u64` above `i64::MAX` and an `i64` below zero must survive a round
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

```text
use std.json.parse

fn accepts(text: string) -> bool:
    match parse(text):
        .Ok(_) => true
        .Err(_) => false

fn checks() -> List[bool]:
    [accepts("[1, 2]"), accepts("[1, 2,]"), accepts("[1, /* c */ 2]")]
    # [true, false, false]: no trailing comma, no comment
```

### Objects

1. r[std-json.parse.object.source-order] The members of a parsed object are in the order of their first occurrence in the text.
2. r[std-json.parse.object.duplicate-last] When a key occurs more than once, the object has one entry for it. Its value is the last one, and its place is the first one's.
3. r[std-json.parse.object.key] A key is a string, and its escapes are decoded as a string's are.

```text
use std.json.parse

fn last_wins() -> string:
    match parse("{\"b\": 1, \"a\": 2, \"b\": 3}"):
        .Ok(value) => "${value}"   # {"b":3,"a":2}: first place, last value
        .Err(_) => "error"
```

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

```text
use std.json.parse

fn decoded() -> string:
    match parse("\"a\\u0041\\n\""):
        .Ok(value) => value.as_text().unwrap_or("")   # "aA" and a line feed
        .Err(_) => ""
```

### Number Syntax

1. r[std-json.parse.number.grammar] A number is `-`, then `0` or a nonzero digit and digits, then an optional fraction, then an optional exponent. A character that breaks this is an `UnexpectedCharacter` error, or `UnexpectedEnd` at the end of the text.
2. r[std-json.parse.number.integer] An integer in the `i64` range or in the `u64` range is exact, as `Number.as_i64` or `Number.as_u64` reads it.
3. r[std-json.parse.number.negative-zero] `-0` is the float `-0.0`, as in `serde_json`, and `0` is the integer zero.
4. r[std-json.parse.number.float] A number with a fraction or an exponent, or an integer outside both ranges, is the float that [`parse_f64`](num.md#r-std-num.parse-f64.decimal-value) gives for its text.
5. r[std-json.parse.number.out-of-range] A float past the finite `f64` range, which `parse_f64` gives as an infinity, is a `NumberOutOfRange` error. The error sits at the number's first character, once its grammar is read to the end.

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

```text
use std.json.parse

fn read_number(text: string) -> string:
    match parse(text):
        .Ok(.Number(n)) => "$n"
        _ => "not a number"

fn demos() -> List[string]:
    [read_number("1.5"), read_number("-0"), read_number("18446744073709551616")]
    # ["1.5", "-0.0", "18446744073709552000.0"]
```

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

```text
use std.json.parse

fn first_error(text: string) -> string:
    match parse(text):
        .Ok(_) => "ok"
        .Err(error) => "$error"

fn demo() -> string:
    first_error("[1,]")   # unexpected character at position 3
```

## JSON Errors

A failed parse reports one of seven errors, each at a position. A failed
decode reports a parse error, or one of three errors at a path:

```text
pub enum JsonError:
    UnexpectedEnd(position: usize)
    UnexpectedCharacter(position: usize)
    InvalidEscape(position: usize)
    LoneSurrogate(position: usize)
    ControlCharacter(position: usize)
    NumberOutOfRange(position: usize)
    NestingTooDeep(position: usize)
    WrongType(path: string, expected: string)
    MissingField(path: string)
    UnknownVariant(path: string, name: string)
```

1. r[std-json.error.variants] `std.json` declares the enum `JsonError` with the ten variants above. The first seven are parse errors, and the last three are decode errors.
2. r[std-json.error.position-bytes] A `position` is a byte offset into the text, counted in bytes of its UTF-8 encoding from 0, as [`slice`](../lang/10-modules.md#r-module.string.byte-offsets) counts.
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

> **Note.** A position is a valid `slice` bound, so
> `text.slice(0, position)` is the text before the error, even after a
> non-ASCII character.

### Decode Errors

A decode error names the value that failed by its path from the root, as
`$.users[2].name`:

| Rule | When |
| --- | --- |
| r[std-json.error.kind.type] `WrongType` | a value of another kind than the type reads, or a number outside the type's range; `expected` names what the type reads |
| r[std-json.error.kind.missing] `MissingField` | an object without the key of a member whose type does not read `null`; `path` ends with that key |
| r[std-json.error.kind.variant] `UnknownVariant` | a variant name that the enum does not declare; `path` is the enum value's, and `name` is the name read |

1. r[std-json.error.path] A `path` is `$`, then one segment per step from the root value. A segment is `.key` for an object's member or a variant's payload, and `[index]` for an array item.
2. r[std-json.error.path.index] An index is written in base ten, counted from 0.
3. r[std-json.error.path.key] A key is written as it is, with no quotes and no escapes.
4. r[std-json.error.display.path] The `Display` text of a decode error contains its `path`.

The `expected` of a `WrongType` depends on the type that reads the value:

| Rule | Type | `expected` |
| --- | --- | --- |
| r[std-json.error.expected.scalar] Scalars | `bool`, `char`, `string`, and each integer and float type | the type's name, as `"u8"` |
| r[std-json.error.expected.array] Lists | `List[T]` | `"array"` |
| r[std-json.error.expected.object] Objects | `Map[string, V]`, a derived data type, and a derived variant's payload | `"object"` |
| r[std-json.error.expected.variant] Enums | a derived enum | `"variant"` |

> **Note.** A path is for people to read. A key that holds `.` or `[`
> makes it ambiguous, as in `serde_path_to_error`.

```text
use std.json.{decode, JsonError}
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize)
data User:
    name: string
    age: i32

fn load(text: string) -> Result[User, JsonError]:
    decode::[User](text)

fn report(text: string) -> string:
    match load(text):
        .Ok(user) => user.name
        .Err(error) => "$error"   # "{\"name\": \"ada\"}" misses $.age
```

## Writing

A `Json` writes as JSON text in two layouts, compact and pretty:

```text
use std.json.{parse, Json}

fn show(value: Json) -> string:
    "${value}"   # {"a":[1,2]}

fn show_pretty(value: Json) -> string:
    value.pretty()
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-json.value.pretty] `pretty` | `pub fn pretty(self) -> string` | the pretty text of `self` |

> **Why.** Text output of a value is a method, as `to_rfc3339()` and
> `to_fixed(n)` are. One spelling is enough, so there is no free
> `pretty` function.

### Compact Text

1. r[std-json.display.compact] `Json` implements `Display`. Its text has no whitespace outside strings: `[1,2]` and `{"a":1}`.
2. r[std-json.display.kinds] `Null` prints as `null`, a `Bool` as `true` or `false`, and a `Number` as [`std-json.number.display`](#r-std-json.number.display) gives.
3. r[std-json.display.containers] An `Array` prints its items in order, and an `Object` prints its members as `"key":value`, each list separated by `,`.
4. r[std-json.display.object-order] An `Object` prints its members in insertion order, in compact text and in `pretty` text. Printing a parsed object gives its keys in the text's order.
5. r[std-json.display.text] A `Text` and each key print in quotes. A quote, a backslash, and each character below U+0020 are escaped, and every other scalar value prints as itself.
6. r[std-json.display.escapes] The escapes are `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, and `\t`, and `\u00` with two lowercase hex digits for the other controls. `/` is not escaped.

```text
use std.json.{Json, parse}

fn compact(value: Json) -> string:
    "$value"   # no whitespace outside strings: [1,2] and {"a":1}

fn demo() -> string:
    match parse("{ \"a\" : [ 1, 2 ] }"):
        .Ok(value) => compact(value)   # {"a":[1,2]}
        .Err(_) => "error"
```

> **Note.** The compact text of a value that `parse` read parses back to an
> equal value.

### Pretty Text

1. r[std-json.pretty.layout] `pretty` writes the compact text's tokens with each item of an array and each entry of an object on its own line. Each line is indented by two spaces for each level of nesting.
2. r[std-json.pretty.separator] An entry is `"key": value`, with one space after the colon. A comma follows every item or entry but the last, with no space before it.
3. r[std-json.pretty.empty] An empty array prints as `[]` and an empty object as `{}`, on one line.
4. r[std-json.pretty.scalar] A value that is not an array or an object prints as its compact text, with no line break.
5. r[std-json.pretty.no-trailing] The text ends with its last bracket, brace, or scalar: no trailing space, no final line break.

| `Json` | `pretty` text |
| --- | --- |
| `[1,2]` | `"[\n  1,\n  2\n]"` |
| `{"a":[]}` | `"{\n  \"a\": []\n}"` |

```text
use std.json.parse

fn pretty_demo(text: string) -> string:
    match parse(text):
        .Ok(value) => value.pretty()
        .Err(_) => ""
```

## Typed JSON

JSON is a format that reads a type's
[serialization consent](../lang/14-annotations.md#serialization).
`to_json` writes any value that implements `Serialize` as a `Json`, and
`from_json` builds a `Deserialize` value from one. `encode` and `decode`
join them to JSON text:

```text
use std.json.{encode, decode, JsonError}
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize)
data Session:
    pub user: string
    token: string
    expires_at: i64

fn save(session: Session) -> string:
    encode(session)   # {"user":"ada","token":"t-81f2","expires_at":1700000000}

fn load(text: string) -> Result[Session, JsonError]:
    decode::[Session](text)
```

### Encode And Decode

| Rule | Function | Result |
| --- | --- | --- |
| r[std-json.to-json] `to_json` | `pub fn to_json[T < Serialize](value: T) -> Json` | the `Json` that `value`'s `serialize` writes |
| r[std-json.from-json] `from_json` | `pub fn from_json[T < Deserialize](value: Json) -> Result[T, JsonError]` | the `T` that `T::deserialize` reads from `value`, or the first error it finds |
| r[std-json.encode] `encode` | `pub fn encode[T < Serialize](value: T) -> string` | the [compact text](#compact-text) of `to_json(value)` |
| r[std-json.decode] `decode` | `pub fn decode[T < Deserialize](text: string) -> Result[T, JsonError]` | `parse(text)`, then `from_json` of the parsed value |

1. r[std-json.decode.parse-error] When `parse` fails, `decode` returns its parse error and reads no value.
2. r[std-json.from-json.errors] `from_json` returns only the decode errors: `WrongType`, `MissingField`, and `UnknownVariant`.
3. r[std-json.from-json.root] Each path that `from_json` reports starts at the value it reads, whose path is `$`.
4. r[std-json.encode.consent] `encode` or `to_json` of a type that does not implement `Serialize` is an error. Error: `unsatisfied-trait-bound`.
5. r[std-json.decode.consent] `decode` or `from_json` into a type that does not implement `Deserialize` is an error. Error: `unsatisfied-trait-bound`.

```text
use std.json.{encode, decode, JsonError}

data Plain:
    id: i32

fn show(value: Plain) -> string:
    encode(value)  # error: unsatisfied-trait-bound

fn load(text: string) -> Result[Plain, JsonError]:
    decode::[Plain](text)  # error: unsatisfied-trait-bound
```

> **Why.** JSON declares no trait of its own, so a type's one consent
> serves JSON and every other format alike, as `serde_json` reads serde's
> `Serialize`.

> **Note.** `encode` writes only the compact layout. For the pretty one,
> write `to_json(value).pretty()`.

### Standard Implementations

JSON writes and reads each standard implementation of
[Serde](serde.md#standard-implementations) as below:

| Rule | Type | `to_json` gives | `from_json` reads |
| --- | --- | --- | --- |
| r[std-json.std.json] Json | `Json` | the value itself | any value, as itself |
| r[std-json.std.bool] Boolean | `bool` | a `Bool` | a `Bool` |
| r[std-json.std.text] String | `string` | a `Text` | a `Text` |
| r[std-json.std.char] Character | `char` | a `Text` of that one character | a `Text` of exactly one character |
| r[std-json.std.integer] Integers | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, and `usize` | the exact integer, as `Number::from_i64` or `Number::from_u64` gives it | a `Number` that is an integer in the type's range |
| r[std-json.std.float] Floats | `f32` and `f64` | the `Number` of the value as an `f64` | any `Number`, as `as_f64` gives it |
| r[std-json.std.optional] Optional | `T?`, where `T` implements the trait | `null` for `.None`, and the payload's `Json` for `.Some` | `.None` from `null`, and `.Some` of what `T` reads from any other value |
| r[std-json.std.list] List | `List[T]`, where `T` implements the trait | an `Array` of the items' values, in order | an `Array`, item by item |
| r[std-json.std.map] Map | `Map[string, V]`, where `V` implements the trait | an `Object` of the entries' values, in the map's iteration order | an `Object`, entry by entry, in its key order |

1. r[std-json.std.json.consent] `std.json` implements `Serialize` and `Deserialize` for `Json`. A `Json` reads through `peek`, so it takes any value.
2. r[std-json.std.integer.read] A float is never an integer, so `1.0` is a `WrongType` for every integer type. So is an integer outside the type's range.
3. r[std-json.std.float-special] A NaN or an infinity has no JSON number, so its `to_json` is `null`, as in `serde_json`.
4. r[std-json.std.float-read] An `f32` reads the nearest `f32` to the number's `f64`. A number past the finite `f32` range is a `WrongType`.
5. r[std-json.std.wrong-kind] Any other kind of value is a `WrongType`, with the `expected` that [Decode Errors](#decode-errors) gives.
6. r[std-json.std.item-path] An error inside a list item gets the segment `[index]`, and one inside a map value gets `.key`.

```text
use std.json.{encode, decode, JsonError}

fn round_trip(names: List[string]) -> Result[List[string], JsonError]:
    decode::[List[string]](encode(names))
```

> **Note.** `T??` loses a layer: `.Some(.None)` writes `null`, which reads
> back as `.None`. A NaN writes `null`, which no float reads.

> **Note.** Only a `Map` with `string` keys has a consent, by
> [`std-serde.std.map-keys`](serde.md#r-std-serde.std.map-keys), so
> `encode` of a `Map[i32, i32]` is an `unsatisfied-trait-bound` error.

### Derived Serialize

JSON writes a data type whose `Serialize` is derived as an object with one
key per member. It writes an enum in the externally tagged form, the
variant's name with its members:

```text
use std.json.encode
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize)
enum Shape:
    Circle(radius: u32)
    Rect(width: i32, height: i32)
    Point(i32, i32)
    Empty
```

| Value | `encode` text |
| --- | --- |
| `Shape.Circle(7)` | `{"Circle":{"radius":7}}` |
| `Shape.Rect(2, 3)` | `{"Rect":{"width":2,"height":3}}` |
| `Shape.Point(4, 5)` | `{"Point":{"_0":4,"_1":5}}` |
| `Shape.Empty` | `"Empty"` |

1. r[std-json.derive.to-json.data] A data type gives an `Object` with one key per member that the derivation walks, in declaration order. The key is the member's name, and its value is the member's `Json`.
2. r[std-json.derive.to-json.enum] A variant with members gives an `Object` with one key, the variant's name. Its value is the `Object` of the variant's members, as for a data type.
3. r[std-json.derive.to-json.unit] A variant with no member in this derivation gives the `Text` of its name.
4. r[std-json.derive.to-json.positional] A positional member's key is its member name: `_0`, `_1`, and so on.
5. r[std-json.derive.to-json.embedded] An embedded member is one key, named by its type's final name, whose value is the part's `Json`. It is not flattened.
6. r[std-json.derive.to-json.shared] Shared constructor data is not a member, so it is not written.
7. r[std-json.derive.to-json.private] A private member is written like any other, since the consent covers it.

> **Note.** A member omitted with `= pass` in a derivation block writes no
> key, as [Omitted Members](../lang/14-annotations.md#omitted-members)
> states. Renames, conditional skips, and defaults per field are open, in
> [Serialization Formats](../../future-work/OPEN_ISSUES.md#serialization-formats).

> **Why.** The externally tagged form is `serde`'s default. It needs no tag
> key that could clash with a member's name.

### Derived Deserialize

JSON reads the form that it writes for a derived `Serialize`:

| Text read as a `Shape` | Result |
| --- | --- |
| `"Empty"` or `{"Empty":{}}` | `.Ok(Shape.Empty)` |
| `{"Rect":{"height":3,"width":2,"z":0}}` | `.Ok(Shape.Rect(2, 3))` |
| `"Rect"` | `.Err(MissingField("$.Rect.width"))` |
| `"Hexagon"` | `.Err(UnknownVariant("$", "Hexagon"))` |
| `{"Rect":3}` | `.Err(WrongType("$.Rect", "object"))` |
| `{"Rect":{},"Empty":{}}` | `.Err(WrongType("$", "variant"))` |

1. r[std-json.derive.from-json.inverse] A derived `Deserialize` reads the form that a derived `Serialize` writes for the same type.
2. r[std-json.derive.from-json.data] A data type reads an `Object`. Any other value is a `WrongType`.
3. r[std-json.derive.from-json.member] It reads each member that the derivation builds, in declaration order, from the value of the member's key. The first error ends the read, with the segment `.name` of the member.
4. r[std-json.derive.from-json.unknown] A key that names no member is ignored.
5. r[std-json.derive.from-json.missing] A missing key reads as `null`: the member's type reads `Json.Null`. When that read fails, the error is a `MissingField` at the path of the missing key.
6. r[std-json.derive.from-json.no-default] A member's declared default is not used for a missing key.
7. r[std-json.derive.from-json.enum] An enum reads a `Text`, or an `Object` with exactly one key. The text or the key is the variant's name. Any other value is a `WrongType`.
8. r[std-json.derive.from-json.name-only] A `Text` reads as an `Object` whose one key is that text and whose value is an empty `Object`.
9. r[std-json.derive.from-json.unknown-variant] A name that is not a variant of the enum is an `UnknownVariant` at the enum value's path.
10. r[std-json.derive.from-json.payload] The variant's payload must be an `Object`, or it is a `WrongType` with the segment `.name` of the variant. Its members then read as a data type's do, under that segment.

```text
use std.json.{decode, JsonError}
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize)
enum Shape:
    Circle(radius: u32)
    Rect(width: i32, height: i32)
    Point(i32, i32)
    Empty

fn read(text: string) -> Result[Shape, JsonError]:
    decode::[Shape](text)

fn shape_of(text: string) -> string:
    match read(text):
        .Ok(Shape.Empty) => "empty"
        .Ok(_) => "a shape with members"
        .Err(error) => "$error"
```

> **Note.** So a member of type `T?` may be missing, and reads `.None`,
> and a member of type `Json` reads `null`. A member of any other
> standard type must be present.

> **Why.** `serde` ignores unknown keys by default, so a reader accepts
> output from a newer writer. A missing optional reads as absent, as
> `serde`'s `Option` does. A declared default is a value for code, not a
> wire rule.

See also: [Encoding](encoding.md#decode-errors), [Num](num.md#integer-parsing), [Collections](collections.md#map-methods), [Numeric Display](../lang/04-type-system.md#numeric-display), [Map Lookup And Order](../lang/04-type-system.md#lookup-and-order), [Serialization](../lang/14-annotations.md#serialization), [Serde](serde.md), [Derived Hashing](hash.md#derived-hashing).
