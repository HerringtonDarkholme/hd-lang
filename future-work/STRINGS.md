# Strings: Go's Byte Model

Status: owner decisions STR1-STR10 (2026-09-29), applied on 2026-09-29.
The specification is authoritative for them. They reverse the
scalar-sequence model of
[Strings](../spec/04-type-system.md#strings) and change
[String Methods](../spec/10-modules.md#string-methods),
[Indexing](../spec/05-expressions.md#indexing),
[Ordering](../spec/05-expressions.md#ordering), and
[Iteration Protocols](../spec/06-control-flow.md#iteration-protocols).
No point waits for the owner ([Still Open](#still-open)).

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

The owner answered the apply pass's points on 2026-09-29 (batch 8). They
are final.

7. **STR7: every bad `slice` offset panics as `index-out-of-bounds`.** This
   covers an offset inside a scalar's encoding, an offset past the end, and
   `a > b`. No new panic category is added.
8. **STR8: "List/Map/string all has Index, string does not have
   IndexSet".** The owner first chose no implementations, then gave this
   final answer.
   - `List[T]` implements `Index[i32]` with `Out = T`, and
     `IndexSet[i32, T]`.
   - `Map[K, V]` implements `Index[K]` with `Out = V`, a read that panics
     on a missing key, and `IndexSet[K, V]`.
   - `string` implements `Index[i32]` with `Out = u8`, and no `IndexSet`.
   - Built-in indexing is unchanged. The implementations let generic code
     index these types through a bound.
9. **STR9: `s[i] = v` is `invalid-assignment-target`.** This confirms the
   applied reading.
10. **STR10: std error positions count bytes,** like every string
    position: `EscapeError.offset`, `ParseNumberError.InvalidDigit`, and
    the like.

STR3 stands as applied: `s[i]` reads one byte as `u8`, in constant time,
as in Go. The owner rejected an `as_bytes()` accessor: "as_bytes will
still somehow alloc in hd". A `List` view of the bytes needs at least a
new header object in Wasm GC, or a copy, while `s[i]` reads the byte with
no allocation.

## Applied

| Decision | Rules |
| --- | --- |
| STR1 | [`types.string.utf8-bytes`](../spec/04-type-system.md#r-types.string.utf8-bytes), [`.valid-utf8`](../spec/04-type-system.md#r-types.string.valid-utf8), [`.literal-utf8`](../spec/04-type-system.md#r-types.string.literal-utf8), [`.concat-bytes`](../spec/04-type-system.md#r-types.string.concat-bytes), [`.from-bytes`](../spec/04-type-system.md#r-types.string.from-bytes), [`.boundary`](../spec/04-type-system.md#r-types.string.boundary), [`.split-scalar`](../spec/04-type-system.md#r-types.string.split-scalar) |
| STR2 | [`types.string.len-bytes`](../spec/04-type-system.md#r-types.string.len-bytes) |
| STR3 | [String Indexing](../spec/05-expressions.md#string-indexing), [`expr.index.trait.read-other`](../spec/05-expressions.md#r-expr.index.trait.read-other); the code `unsupported-string-indexing` is withdrawn |
| STR4 | [`flow.for.string-not-iterable`](../spec/06-control-flow.md#r-flow.for.string-not-iterable), [`flow.for.string-explicit`](../spec/06-control-flow.md#r-flow.for.string-explicit), [`module.string.chars`](../spec/10-modules.md#r-module.string.chars), [`.char-indices`](../spec/10-modules.md#r-module.string.char-indices), [`.bytes`](../spec/10-modules.md#r-module.string.bytes) |
| STR5 | [`module.string.slice`](../spec/10-modules.md#r-module.string.slice), [`.shared`](../spec/10-modules.md#r-module.string.slice.shared); the boundary rule is now STR7's |
| STR6 | [`module.string.byte-offsets`](../spec/10-modules.md#r-module.string.byte-offsets), [`module.method.i32-bytes`](../spec/10-modules.md#r-module.method.i32-bytes), [`types.string.compare-bytes`](../spec/04-type-system.md#r-types.string.compare-bytes), [`expr.ord.std.string-bytes`](../spec/05-expressions.md#r-expr.ord.std.string-bytes), [`types.string.host-bytes`](../spec/04-type-system.md#r-types.string.host-bytes) |
| STR7 | [`module.string.slice.bad-offset`](../spec/10-modules.md#r-module.string.slice.bad-offset), [`module.string.slice.reversed`](../spec/10-modules.md#r-module.string.slice.reversed); `module.string.slice.boundary` is retired |
| STR8 | [Built-In Implementations](../spec/05-expressions.md#built-in-implementations): [`expr.index.std.list`](../spec/05-expressions.md#r-expr.index.std.list), [`.map`](../spec/05-expressions.md#r-expr.index.std.map), [`.string`](../spec/05-expressions.md#r-expr.index.std.string), [`.map-read`](../spec/05-expressions.md#r-expr.index.std.map-read), [`expr.index.trait.builtin-direct`](../spec/05-expressions.md#r-expr.index.trait.builtin-direct); `expr.index.trait.builtin-string` is retired |
| STR9 | [`expr.index.string.no-assign`](../spec/05-expressions.md#r-expr.index.string.no-assign), unchanged |
| STR10 | Not in the specification, which names no std error position; the library change is in [`std.text`](STDLIB.md#stdtext) |

`string::from_utf8` and `Utf8Error` are std items outside the
specification's scope; they are in [`std.text`](STDLIB.md#stdtext), with
the library changes that STR1-STR6 need. The specification states only
that a conversion from bytes checks validity.

## Still Open

None. STR7-STR10 answered the five points the STR1-STR6 apply pass
raised.
