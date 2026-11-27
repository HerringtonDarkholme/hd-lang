# `std.format`

Generated from `hd doc std.format`. Run `hd doc std.format.NAME` in a package for one item's full text.

## `Display`

```text
pub trait Display < ?Copy
```

The canonical text trait: string interpolation writes a value through it.

- `fn write(&self, out: &mut StringBuilder) -> void`: Writes the value's text to `out`; the default pushes `to_string()`.

## `Debug`

```text
pub trait Debug < ?Copy
```

The debug-text trait: `debug(value)` and `dbg` write a value through it, field by field.

## `DebugWriter`

```text
pub struct DebugWriter
```

What a `Debug` implementation writes to: one of the builders, or custom text.

- `pub fn write(&mut self, text: string) -> void`: Writes custom text, for a value that no builder fits.
- `pub fn debug_struct(*mut self, name: string) -> DebugStruct`: Starts a named value with named fields.
- `pub fn debug_tuple(*mut self, name: string) -> DebugTuple`: Starts a named value with positional fields.
- `pub fn debug_list(*mut self) -> DebugList`: Starts a sequence.
- `pub fn debug_map(*mut self) -> DebugMap`: Starts key-value pairs.

## `DebugStruct`

```text
pub struct DebugStruct
```

The builder `debug_struct` returns: `field` per field, then `finish`.

- `pub fn field[T < Debug](&mut self, name: string, value: T) -> &mut DebugStruct`: Adds one named field.
- `pub fn finish(&mut self) -> void`: Ends the value the builder writes.

## `DebugTuple`

```text
pub struct DebugTuple
```

The builder `debug_tuple` returns: `field` per field, then `finish`.

- `pub fn field[T < Debug](&mut self, value: T) -> &mut DebugTuple`: Adds one positional field.
- `pub fn named_field[T < Debug](&mut self, name: string, value: T) -> &mut DebugTuple`: Adds a named argument `name=value` after the positional fields.
- `pub fn finish(&mut self) -> void`: Ends the value the builder writes.

## `DebugList`

```text
pub struct DebugList
```

The builder `debug_list` returns: `entry` per item, then `finish`.

- `pub fn entry[T < Debug](&mut self, value: T) -> &mut DebugList`: Adds one item.
- `pub fn finish(&mut self) -> void`: Ends the value the builder writes.

## `DebugMap`

```text
pub struct DebugMap
```

The builder `debug_map` returns: `entry` per pair, then `finish`.

- `pub fn entry[K < Debug, V < Debug](&mut self, key: K, value: V) -> &mut DebugMap`: Adds one key-value pair.
- `pub fn finish(&mut self) -> void`: Ends the value the builder writes.

## `debug`

```text
pub fn debug[T < Debug](value: T) -> string
```

Returns the text that `Debug` writes for `value`: stable, field by field, multi-line, and consistently indented.

## `dbg`

```text
pub fn dbg[Args < Tuple](values...: Args) -> void
```

Prints each value's debug text with its call site, for troubleshooting.
