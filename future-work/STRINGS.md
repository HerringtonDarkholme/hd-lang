# Strings: Go's Byte Model

Status: owner decisions STR1-STR6 (2026-09-29), not yet applied. They
reverse the scalar-sequence model of
[Strings](../spec/04-type-system.md#strings) and change
[String Methods](../spec/10-modules.md#string-methods),
[Indexing](../spec/05-expressions.md#indexing),
[Ordering](../spec/05-expressions.md#ordering), and
[Iteration Protocols](../spec/06-control-flow.md#iteration-protocols).
Points that need the owner are under [Still Open](#still-open).

> **Owner (2026-09-29).** "i regret one thing. hd lang's string. I think we
> should follow golang here. make the representation simple, but perf not
> too bad. esp. len/range"

## Owner Decisions

Decided 2026-09-29. They are final.

1. **STR1: a `string` is an immutable sequence of bytes that is always
   valid UTF-8.** This is Go's layout with the validity guarantee of Rust
   and Swift.
   - Literals and concatenation are valid by construction.
   - Conversion from bytes checks validity:
     `string::from_utf8(bytes) -> Result[string, Utf8Error]`, in std.
   - A byte-offset operation that would split a scalar panics.
2. **STR2: `s.len()` is the byte count,** in constant time.
3. **STR3: `s[i]` is the byte at offset `i`, as `u8`,** in constant time.
   An index out of range panics as a list index does.
4. **STR4: `string` does not implement `Iterable`.** `for c in s` is an
   error: `unsatisfied-trait-bound`, by `flow.for.not-iterable`.
   - Iteration is explicit: `s.chars()` yields `char`, `s.char_indices()`
     yields `(byte offset, char)` like Go's `range`, and `s.bytes()` yields
     `u8`.
   - Reason: `len` and `s[i]` count bytes, so an implicit character loop
     would silently treat a string as characters, as Rust's model does.
5. **STR5: a substring is `s.slice(a, b)`, with byte offsets,** in
   constant time, sharing the original's bytes. An offset that is not on a
   scalar boundary panics. There is no slice syntax.
6. **STR6: positions are byte offsets.**
   - Positions that string methods return, such as `find`'s, are byte
     offsets.
   - Equality and ordering compare bytes. For valid UTF-8 this is the same
     order as scalar order.
   - Strings cross the host boundary as their UTF-8 bytes, with no
     conversion.
   - Retired: `types.string.len` (scalar count), `types.string.no-indexing`,
     and the Note that `len` takes linear time.

## Still Open

Each point waits for the owner.

| # | Point | **Recommendation** |
| --- | --- | --- |
| 1 | Which panic category does `s.slice(a, b)` use when an offset is not a scalar boundary or is past the end? The category set in [`flow.panic.category-set`](../spec/06-control-flow.md#r-flow.panic.category-set) is closed, and none names a boundary. | `index-out-of-bounds`: the offset is outside the positions the string accepts, and no new category is needed. |
| 2 | What does `s.slice(a, b)` do when `a > b`, with both on scalar boundaries? STR5 names only the boundary case. | Panic with the same category, as Go and Rust do. |
