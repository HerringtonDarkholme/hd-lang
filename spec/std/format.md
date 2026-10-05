# Format

Status: standard library specification draft.

This chapter defines the part of `std.format` that `lib/std` writes in
ordinary hd over the language tier:

- the text that the prelude function `debug` returns;
- the `DebugWriter` builders and their layout;
- the builder calls that `@derive(Debug)` generates;
- which public `std` types implement `Debug`.

The language tier keeps what the compiler knows by name
([Debug Trait](../lang/09-traits.md#debug-trait)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Display`, `Debug` | prelude traits; `assert_equal` requires `Debug` ([`module.testing.assert-equal-debug`](../lang/10-modules.md#r-module.testing.assert-equal-debug)) |
| `debug`'s signature | a prelude function ([`module.prelude.debug`](../lang/10-modules.md#r-module.prelude.debug)) |
| `DebugWriter` | `Debug`'s one method names it in its signature |
| `@derive(Debug)` | a derivation through a [template](../lang/14-annotations.md#templates) |
| `dbg` | a prelude function with a compiler-supplied body ([`module.prelude.dbg`](../lang/10-modules.md#r-module.prelude.dbg)), which prints through `Debug` where a type implements it |

## Debug Text

1. r[std-format.debug.render] The prelude function `debug(value)` returns the text that `Debug` writes for `value`: stable, field by field, multi-line, and consistently indented.
2. r[std-format.debug.render.whole] `debug(value)` applies none of the limits of [Large Values](../lang/10-modules.md#large-values), which only `dbg` applies, so it returns the whole text.
3. r[std-format.debug.source] `Debug` writes a value the way hd source writes it, so a printed value pastes back as hd code and builds an equal value, wherever each of its parts has a source form.
4. r[std-format.debug.source.no-context] The text needs no expected type: a variant is written qualified by its enum, never as the `.Variant` shorthand.
5. r[std-format.debug.source.markers] A part with no source form writes a marker instead: `<fn ...>` for a function, `<handle>` for a runtime handle, and `<cycle>` and the `…` limits of `dbg` for a large value. Such text is not source.

| Rule | Value | Text |
| --- | --- | --- |
| r[std-format.debug.source.data] Data | a `data` value | `Point { x: 1, y: 2 }`, and `Empty {}` without fields |
| r[std-format.debug.source.variant] Enum variant | a variant of an enum | the qualified variant, then a call's arguments: positional payloads first, then named ones as `name=value`, as in `Shape.Dot`, `Shape.Pair(1, 2)`, `Shape.Circle(radius=2.0)`, and `Shape.Mixed(1, label="x")` |
| r[std-format.debug.source.prelude] Option and Result | `T?` and `Result[T, E]` | `Option.Some(1)`, `Option.None`, `Result.Ok(1)`, and `Result.Err("bad")`, as [`data.prelude.values`](../lang/08-data-and-enums.md#r-data.prelude.values) spells them |
| r[std-format.debug.source.newtype] Newtype | a newtype value | its constructor call, as in `Meters(2.5)`, unless it derives `Debug`, which writes the base value as [`trait.derive.newtype.templated`](../lang/09-traits.md#r-trait.derive.newtype.templated) says |
| r[std-format.debug.source.tuple] Tuple | a tuple | its elements in parentheses, as in `(1, "a")`, `(1,)`, and `()` |
| r[std-format.debug.source.list] List | a `List[T]` | its elements in brackets, as in `[1, 2]`, and `[]` when empty |
| r[std-format.debug.source.map] Map | a `Map[K, V]` | its entries in braces, as in `{"a": 1}` |
| r[std-format.debug.source.string] String | a `string` | a string literal that escapes `\`, `"`, and `$` with a backslash, writes a line feed, carriage return, tab, and NUL as `\n`, `\r`, `\t`, and `\0`, and writes another control character as `\u{HEX}` |
| r[std-format.debug.source.char] Char | a `char` | a char literal that escapes `\` and `'` the same way and writes a control character as in a string, as in `'x'` and `'\n'` |

```text
enum Shape:
    Dot
    Circle(radius: f64)

fn shown() -> string:
    debug(Shape.Circle(radius=2.0))  # "Shape.Circle(radius=2.0)", which is valid hd
```

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
| r[std-format.debug.builder.named-field] Named argument | `.named_field(arg_name, value)` on a `debug_tuple` builder, after its `field` calls | a named argument `arg_name=value` of the call that `debug_tuple` writes |
| r[std-format.debug.builder.list] List | `out.debug_list()`, then `.entry(value)` per item, then `.finish()` | a sequence |
| r[std-format.debug.builder.map] Map | `out.debug_map()`, then `.entry(key, value)` per pair, then `.finish()` | key-value pairs |
| r[std-format.debug.builder.write] Write | `out.write(text)` | custom text, for a value that no builder fits |

1. r[std-format.debug.builder.types] `std.format` declares the builder types `DebugStruct`, `DebugTuple`, `DebugList`, and `DebugMap`, which the calls above return. None is a prelude name.
2. r[std-format.debug.builder.values] Each `field` and `entry` value, and each `entry` key, must implement `Debug`. The builder writes it through its own `debug`.
3. r[std-format.debug.builder.chain] `field` and `entry` return their builder, so calls chain, and `finish` ends the value.
4. r[std-format.debug.builder.tuple.unnamed] `debug_tuple("")` describes a tuple, so it writes no name. One field gets a comma after it, as in `(1,)`, as a tuple's `Display` text does.
5. r[std-format.debug.layout] The writer chooses a compact or a pretty layout. An implementation's builder calls are the same for both.
6. r[std-format.debug.derive-builders] `@derive(Debug)` generates builder calls, as a hand-written implementation writes them, so derived and hand-written text share one layout.
7. r[std-format.debug.derive-builders.source] The derived calls write the value as hd source, by the shape of each value, as the table below states. A qualified variant name is the enum's name, a dot, and the variant's name.

| Rule | Value | Derived calls |
| --- | --- | --- |
| r[std-format.debug.derive-builders.data] Data type | a value of a `data` type, with or without fields | `out.debug_struct(type_name)`, then `.field(field_name, value)` per field in declaration order, then `.finish()` |
| r[std-format.debug.derive-builders.variant.named] Named variant | a variant whose payload fields are all named | `out.debug_tuple(qualified_name)`, then `.named_field(field_name, value)` per payload field in order, then `.finish()`, printing `Shape.Circle(radius=2.0)` |
| r[std-format.debug.derive-builders.variant.positional] Positional variant | a variant whose payload fields are all positional | `out.debug_tuple(qualified_name)`, then `.field(value)` per payload value in order, then `.finish()` |
| r[std-format.debug.derive-builders.variant.unit] Unit variant | a variant without a payload | `out.write(qualified_name)` only |
| r[std-format.debug.derive-builders.variant.mixed] Mixed variant | a variant with both positional and named payload fields, such as `Mixed(i32, label: string)` | `out.debug_tuple(qualified_name)`, then `.field(value)` per positional field, then `.named_field(field_name, value)` per named field, then `.finish()`, printing `Mixed.Mixed(1, label="x")` |

1. r[std-format.debug.tuple-template] `Debug`'s [tuple template](../lang/14-annotations.md#tuple-templates) calls `out.debug_tuple("")`, then `.field(value)` per element, then `.finish()`. Its walker implements `rest`, so a rest element's items are fields too.

> **Why.** `assert_equal` and property tests show failing values through
> `Debug`, so any type a test compares can show itself without a
> user-facing `Display`. Builders keep the layout in the writer: plain
> writes would fix it in each implementation, which leaves no pretty mode
> or depth limit and lets derived and hand-written text drift apart.

> **Note.** The line breaks and indentation of `debug` text are
> standard-library API. The syntax of each value is specified by
> [`std-format.debug.source`](#r-std-format.debug.source), and fixtures
> assert only short values that fit on one line.

See also: [Debug Trait](../lang/09-traits.md#debug-trait),
[Standard Testing](../lang/10-modules.md#standard-testing),
[Typed Derivation](../lang/14-annotations.md#typed-derivation).

## Debug For Standard Types

The public types of `std` implement `Debug`, so a test can compare and
show them:

```text
use std.time.{Duration, s}

fn shown(timeout: Duration) -> string:
    debug(timeout)
```

1. r[std-format.debug.std-types] Every public data type, enum, and newtype that `std` declares implements `Debug` when the type of each of its members implements `Debug`.
2. r[std-format.debug.std-types.generic] A generic one implements `Debug` under the bounds that its members need, as in `impl[T < Debug] Debug for Reverse[T]`.
3. r[std-format.debug.std-types.calls] Its builder calls are those that `@derive(Debug)` generates for its declaration, by [`std-format.debug.derive-builders.source`](#r-std-format.debug.derive-builders.source). A newtype with `@derive(Debug)` writes its base value, as [`trait.derive.newtype.templated`](../lang/09-traits.md#r-trait.derive.newtype.templated) gives. A std newtype with a hand-written `Debug`, such as `Path` or `ExitCode`, writes its constructor call.
4. r[std-format.debug.std-types.sequences] `Deque` and `Heap` are exceptions to [`std-format.debug.std-types.calls`](#r-std-format.debug.std-types.calls): each writes one `debug_list` entry per element, as `List` does. A `Deque` writes them from front to back, and a `Heap` in an order this chapter does not specify.
5. r[std-format.debug.std-types.set] `Set` is one more exception: it writes one `debug_list` entry per element, in iteration order.
6. r[std-format.debug.std-types.exempt-members] A type with a member that holds a function value, or a trait value whose trait does not extend `Debug`, does not implement `Debug`. Examples are `Iterator[T]` and `Choices`.
7. r[std-format.debug.std-types.context-error] `ContextError` is the exception to [`std-format.debug.std-types.exempt-members`](#r-std-format.debug.std-types.exempt-members): it implements `Debug` by [`std-error.context.debug`](error.md#r-std-error.context.debug), though it holds an erased `Error`.
8. r[std-format.debug.std-types.time] `Duration`, `Timestamp`, and `Instant` are exceptions to [`std-format.debug.std-types.calls`](#r-std-format.debug.std-types.calls): each writes text with `write`, so `debug` shows a `Duration` as its `Display` text (`250ms`), a `Timestamp` as its RFC 3339 text, and an `Instant` as `Instant(5ms)`.

> **Note.** The derived calls name a type's private fields too. Like all
> `debug` text, that text is not portable, and fixtures do not depend on it.

> **Why.** `assert_equal` and property tests show values through `Debug`.
> A std type without it could not be compared in a test, and
> [`trait.own.rule`](../lang/09-traits.md#r-trait.own.rule) keeps a caller
> from adding the implementation.
