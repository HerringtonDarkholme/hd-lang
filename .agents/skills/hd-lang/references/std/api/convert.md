# `std.convert`

Generated from `hd doc std.convert`. Run `hd doc std.convert.NAME` in a package for one item's full text.

## `From`

```text
pub trait From[T]
```

The general conversion trait: `from(value)` builds a `Self` from a `T`, and postfix `?` finds it without an import.

## `TryFrom`

```text
pub trait TryFrom[T]
```

An explicit conversion that returns an error when the source cannot be represented.

## `CharTryFromError`

```text
pub struct CharTryFromError
```

A scalar conversion failed because the number is not a Unicode scalar value.
