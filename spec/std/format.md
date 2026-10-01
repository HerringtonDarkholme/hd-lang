# Format

Status: standard library specification draft.

This chapter defines the part of `std.format` that `lib/std` writes in
ordinary hd over the language tier:

- the text that the prelude function `debug` returns;
- the `DebugWriter` builders and their layout;
- the builder calls that `@derive(Debug)` generates.

The language tier keeps what the compiler knows by name
([Debug Trait](../09-traits.md#debug-trait)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Display`, `Debug` | prelude traits; `assert_equal` requires `Debug` ([`module.testing.assert-equal-debug`](../10-modules.md#r-module.testing.assert-equal-debug)) |
| `debug`'s signature | a prelude function ([`module.prelude.debug`](../10-modules.md#r-module.prelude.debug)) |
| `DebugWriter` | `Debug`'s one method names it in its signature |
| `@derive(Debug)` | a derivation through a [template](../14-annotations.md#templates) |

## Debug Text

1. r[std-format.debug.render] The prelude function `debug(value)` returns the text that `Debug` writes for `value`: stable, field by field, multi-line, and consistently indented.

## Debug Builders

`DebugWriter` describes a value through builders, like Rust's `Formatter`:

```text
use std.format.DebugWriter

data Point:
    x: i32
    y: i32

impl Debug for Point:
    fn debug(self, out: mut DebugWriter) -> void:
        out.debug_struct("Point").field("x", self.x).field("y", self.y).finish()
```

| Rule | Call | Describes |
| --- | --- | --- |
| r[std-format.debug.builder.struct] Struct | `out.debug_struct(name)`, then `.field(field_name, value)` per field, then `.finish()` | a named value with named fields |
| r[std-format.debug.builder.tuple] Tuple | `out.debug_tuple(name)`, then `.field(value)` per field, then `.finish()` | a named value with positional fields |
| r[std-format.debug.builder.list] List | `out.debug_list()`, then `.entry(value)` per item, then `.finish()` | a sequence |
| r[std-format.debug.builder.map] Map | `out.debug_map()`, then `.entry(key, value)` per pair, then `.finish()` | key-value pairs |
| r[std-format.debug.builder.write] Write | `out.write(text)` | custom text, for a value that no builder fits |

1. r[std-format.debug.builder.types] `std.format` declares the builder types `DebugStruct`, `DebugTuple`, `DebugList`, and `DebugMap`, which the calls above return. None is a prelude name.
2. r[std-format.debug.builder.values] Each `field` and `entry` value, and each `entry` key, must implement `Debug`. The builder writes it through its own `debug`.
3. r[std-format.debug.builder.chain] `field` and `entry` return their builder, so calls chain, and `finish` ends the value.
4. r[std-format.debug.builder.tuple.unnamed] `debug_tuple("")` describes a tuple, so it writes no name. One field gets a comma after it, as in `(1,)`, as a tuple's `Display` text does.
5. r[std-format.debug.layout] The writer chooses a compact or a pretty layout. An implementation's builder calls are the same for both.
6. r[std-format.debug.derive-builders] `@derive(Debug)` generates builder calls, as a hand-written implementation writes them, so derived and hand-written text share one layout.
7. r[std-format.debug.derive-builders.mapping] The derived calls follow Rust's `#[derive(Debug)]`, by the shape of each value, as the table below states.

| Rule | Value | Derived calls |
| --- | --- | --- |
| r[std-format.debug.derive-builders.data] Data type | a value of a `data` type, with or without fields | `out.debug_struct(type_name)`, then `.field(field_name, value)` per field in declaration order, then `.finish()` |
| r[std-format.debug.derive-builders.record] Record variant | a variant whose payload fields are all named | `out.debug_struct(variant_name)`, then `.field(field_name, value)` per payload field in order, then `.finish()` |
| r[std-format.debug.derive-builders.tuple] Tuple variant | a variant whose payload fields are all positional | `out.debug_tuple(variant_name)`, then `.field(value)` per payload value in order, then `.finish()` |
| r[std-format.debug.derive-builders.unit] Unit variant | a variant without a payload | `out.write(variant_name)` only |
| r[std-format.debug.derive-builders.mixed] Mixed variant | a variant with both positional and named payload fields, such as `Mixed(i32, label: string)` | `out.debug_struct(variant_name)`, then `.field(field_name, value)` per payload field in order, where a positional field is named `_0`, `_1`, and so on by its position, then `.finish()`, printing `Mixed { _0: 1, label: "x" }` |

1. r[std-format.debug.tuple-template] `Debug`'s [tuple template](../14-annotations.md#tuple-templates) calls `out.debug_tuple("")`, then `.field(value)` per element, then `.finish()`. Its walker implements `rest`, so a rest element's items are fields too.

> **Why.** `assert_equal` and property tests show failing values through
> `Debug`, so any type a test compares can show itself without a
> user-facing `Display`. Builders keep the layout in the writer: plain
> writes would fix it in each implementation, which leaves no pretty mode
> or depth limit and lets derived and hand-written text drift apart.

> **Note.** The exact layout of `debug` text is standard-library API.
> Portable code and conformance fixtures do not depend on that text.

See also: [Debug Trait](../09-traits.md#debug-trait),
[Standard Testing](../10-modules.md#standard-testing),
[Typed Derivation](../14-annotations.md#typed-derivation).
