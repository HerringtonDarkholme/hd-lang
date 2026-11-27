# `std.encoding.json`

Generated from `hd doc std.encoding.json`. Run `hd doc std.encoding.json.NAME` in a package for one item's full text.

## `Json`

```text
pub enum Json
```

A JSON value: `null`, `Bool`, `Number`, `Text`, `Array`, or `Object`.

- `pub fn pretty(&self) -> string`: The pretty text of the value: each item of an array and each entry of an object on its own line, indented by two spaces for each level of nesting.
- `pub fn get(&self, key: string) -> Json?`: The value of `key` when the value is an `Object` with that key.
- `pub fn at(&self, index: usize) -> Json?`: The item at `index` when the value is an `Array` and `index` is from 0 up to its length.
- `pub fn as_text(&self) -> string?`: The string of a `Text` value.
- `pub fn as_bool(&self) -> bool?`: The value of a `Bool`.
- `pub fn is_null(&self) -> bool`: `true` exactly for `Null`.
- `pub fn as_i64(&self) -> i64?`: The integer of a `Number` that is an integer in the `i64` range; a float, even `1.0`, gives `.None`.
- `pub fn as_u64(&self) -> u64?`: The integer of a `Number` that is an integer from zero up in the `u64` range; a float gives `.None`.
- `pub fn as_f64(&self) -> f64?`: The `f64` of any `Number`, an integer converted to the nearest `f64`.
- `pub fn pointer(&self, path: string) -> Json?`: The value at the JSON Pointer `path` (RFC 6901), such as `/db/pool/0`, or `.None` when no value is there.

## `JsonObject`

```text
pub struct JsonObject
```

The members of a JSON object, in insertion order.

- `pub fn new() -> JsonObject`: Returns an object with no members.
- `pub fn len(&self) -> usize`: Returns the number of members.
- `pub fn is_empty(&self) -> bool`: True exactly when the object has no members.
- `pub fn get(&self, key: string) -> Json?`: The value of `key`, or `.None` when the object has no such key.
- `pub fn contains_key(&self, key: string) -> bool`: True exactly when the object has the key.
- `pub fn set(&mut self, key: string, value: Json) -> void`: Sets `key` to `value`. A new key goes last, and an existing key keeps its place.
- `pub fn remove(&mut self, key: string) -> Json?`: Removes `key` and returns its value, or `.None` when the object has no such key. The other members keep their order.
- `pub fn keys(&self) -> List[string]`: The keys, in insertion order.

## `JsonObjectIter`

```text
pub struct JsonObjectIter
```

A cursor over the members of a JSON object, in insertion order.

## `Number`

```text
pub struct Number
```

A JSON number, exact for 64-bit integers: integers in the `i64` and `u64` ranges are exact, and every other number is an `f64`.

- `pub fn from_i64(value: i64) -> Number`: Returns the integer `value` as a `Number`.
- `pub fn from_u64(value: u64) -> Number`: Returns the integer `value` as a `Number`.
- `pub fn from_f64(value: f64) -> Number?`: Returns the float `value` as a `Number`, or `.None` for a NaN or an infinity.
- `pub fn as_i64(&self) -> i64?`: The integer when it is in the `i64` range, and `.None` for a float.
- `pub fn as_u64(&self) -> u64?`: The integer when it is from zero up, and `.None` for a negative integer or a float.
- `pub fn as_f64(&self) -> f64?`: `.Some` of the float, or of the integer converted to the nearest `f64`.

## `JsonError`

```text
pub enum JsonError
```

The error that `parse` and `decode` return: seven parse errors at a byte position, and four decode errors at a path.

## `parse`

```text
pub fn parse(text: string) -> Result[Json, JsonError]
```

Reads one JSON text, strictly as RFC 8259 defines it, and returns `.Ok` of the value or `.Err` of the first error.

## `to_json`

```text
pub fn to_json[T < Serialize](value: T) -> Json
```

The `Json` that `value`'s `serialize` writes.

## `from_json`

```text
pub fn from_json[T < Deserialize](value: Json) -> Result[T, JsonError]
```

The `T` that `T::deserialize` reads from `value`, or the first error it finds.

## `encode`

```text
pub fn encode[T < Serialize](value: T) -> string
```

The compact JSON text of `value`, with no whitespace outside strings.

## `encode_pretty`

```text
pub fn encode_pretty[T < Serialize](value: T) -> string
```

The pretty JSON text of `value`: `to_json(value).pretty()`.

## `decode`

```text
pub fn decode[T < Deserialize](text: string) -> Result[T, JsonError]
```

Reads a typed value from JSON text in one pass.
