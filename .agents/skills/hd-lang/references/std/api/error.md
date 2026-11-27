# `std.error`

Generated from `hd doc std.error`. Run `hd doc std.error.NAME` in a package for one item's full text.

## `Error`

```text
pub trait Error < Display & Inspectable
```

The standard error trait: a displayable, inspectable failure that may name its source.

## `ErrorReport`

```text
pub struct ErrorReport
```

A boundary-safe report of an error: its message and the messages of its sources.

- `pub message: string`: The error's own message.
- `pub sources: List[string]`: The messages of the error's sources, in chain order.

## `report_of`

```text
pub fn report_of(error: &dyn Error) -> ErrorReport
```

Builds the boundary-safe report of `error`: its message, and its sources' messages.

## `message`

```text
pub fn message(text: string) -> *dyn Error
```

Makes an error whose `Display` text is `text` and that has no source.
