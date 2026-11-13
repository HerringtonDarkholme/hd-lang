# List Blocks: One Pointer To One Block

Design record, 2026-11-12; domain: `std.rt.memory`, `std.collections`,
the ownership pass, the Wasm backend. It records owner decisions on
[#1219](https://github.com/HerringtonDarkholme/hd-lang/issues/1219) that
are not in the specification yet, and the questions still open. Nothing
here is accepted behavior until the spec says so. The stress test is
[LIST_BLOCK_STRESS_TEST.md](LIST_BLOCK_STRESS_TEST.md).

## Contents

- [Context](#context)
- [Decisions](#decisions)
- [The Block](#the-block)
- [Operations](#operations)
- [Open Questions](#open-questions)
- [Cost](#cost)
- [Prior Art](#prior-art)
- [Superseded Shapes](#superseded-shapes)

## Context

`List[T]` is a compiler built-in today: intrinsics, helpers and methods
that only the compiler knows. `UniqList[T]` already writes the same
storage in hd over [`Buffer`](../../spec/lang/10-modules.md#typed-memory).
#1219 moves `List` into std; #1220 does the same for `Map`.

The first shape, a box holding a `UniqList`, cost a second hop per
access. Lanes 46 to 48 left its write loop at 78 ms against the built-in
List's 19 ms. The owner dropped it: "using double jump is a dumb idea".

## Decisions

All by the owner on 2026-11-12, on #1219.

| ID | Decision | Source |
| --- | --- | --- |
| D1 | `List[T < Copy]` (counted, copy-on-write) and `UniqList[T < ?Copy]` (one owner, no count) each hold one pointer to one block. No second hop. | "using double jump is a dumb idea"; "let's try one-word list" |
| D2 | `box` takes unsized data: a std-only struct whose last field is an unsized tail, so the head and the elements are one allocation. No separate ManagedBuffer abstraction. | #1219, shape change |
| D3 | The fallback, if a one-word hd `List` misses 5% of the built-in List on the probe, is three words `{store: *[T], len, cap}`. | "let's try one-word list" |
| D4 | The tail is raw, std-only room. The language knows only its slot count, to allocate and free, and treats the slots as empty. Std's drop hook drops the first `len` items; the record says `List[T < Copy]` drops none. | answered "i" |
| D5 | Only std may declare a struct with a tail. | answered "i" |
| D6 | `box(head, slots = n)` creates a block with `n` empty slots. `regrow(r: &mut *S, slots)` grows it in place when the box is unshared and the allocator can, else moves to a new block and releases the old one. Both work on the counted and the uncounted box. | answered "i" |
| D7 | `List` holds a counted box. `UniqList` holds an uncounted box with no count header, 8 bytes smaller per block. Converting between them allocates and copies. | answered "ii"; "uniqList is using something like uncounted box" |
| D8 | Kept from main: `unique_mut`, `make_unique(r, copy)` with its loop hoist, `box` without `'static`, and the release header-load hoist (#1706). | e1689a211, e6858eaa3, 94bd8e55c |
| D9 | The compiler work is making the built-in List's facts general for any counted pointer to a block with a header: one copy-on-write check before a loop, header loads hoisted out of loops that leave the list alone, bounds proven against that length. | "let's try one-word list" |
| D10 | `Map` (#1220) reuses the same design and the same uniqueness test. | gap 1 of 3 |

## The Block

A counted block, as today's built-in List buffer
([DESIGN.md §23.4](../../crates/hd_wasm/DESIGN.md#234-counted-objects),
[§23.7](../../crates/hd_wasm/DESIGN.md#237-strings-and-collection-buffers))
less its owner and padding words. Offsets are wasm32 bytes from the block.

| Offset | Counted (`List`) | Uncounted (`UniqList`) |
| --- | --- | --- |
| 0 | count, 4 bytes | `len` |
| 4 | descriptor word, 4 bytes | `cap` |
| 8 | `len` (the pointer points here) | slot 0, for an 8-aligned `T` |
| 12 | `cap` | |
| 16 | slot 0 | |

The value of a `List` or a `UniqList` is that one address. Today's
built-in block has a 24-byte header (count, descriptor, capacity, length,
owner, padding); the counted block's is 16 bytes.

The std side, as the record pictures it. The tail's spelling is open
([O2](#open-questions)); `Slots[T]` is a placeholder.

```text
# std.collections; assumed spelling of the tail (O2)
struct ListStore[T]:
    len: usize
    cap: usize
    items: Slots[T]

pub struct List[T]:
    store: *ListStore[T]

struct UniqStore[T < ?Copy]:
    len: usize
    cap: usize
    items: Slots[T]
```

## Operations

```text
# a push, with every decided operation; the uncounted form is open (O1)
impl[T] List[T]:
    pub fn push(&mut self, value: T) -> void:
        let s = make_unique(&mut self.store, copy_store)
        if s.len == s.cap:
            need := grown(s.cap)
            regrow(&mut self.store, slots = need)
            s = make_unique(&mut self.store, copy_store)
        s.items.at(s.len).write(value)
        s.len += 1
```

- `make_unique` tests the count once and, when shared, stores a copy.
  The ownership pass runs it once before a loop that leaves the place
  alone ([`module.rt.make-unique.copy`](../../spec/lang/10-modules.md#r-module.rt.make-unique.copy)).
- `regrow` keeps the head and the slots' bytes. Only std knows which
  slots hold values, so std writes `cap` after it.
- The block's release glue calls the head type's std `Drop` hook, then
  frees the block.

## Open Questions

The owner named these as not decided. The stress test gives two to four
options for each, priced, with a labeled recommendation.

| ID | Question |
| --- | --- |
| O1 | The uncounted box: its type and how it is created. |
| O2 | How a std struct declares its tail field. |
| O3 | Where the slot count lives for freeing. |
| O4 | How std reaches slot `i`. |
| O5 | `regrow` on an uncounted box. |
| O6 | Weak references and `ref_identity` on a tail block. |

The stress test found more that is unsettled. Three decided points do
not hold as written: `make_unique`'s `copy` cannot return an unsized
head, `regrow` on a shared box would release twice, and a
`List[string]` must drop its items. See its
[Problems, Ranked](LIST_BLOCK_STRESS_TEST.md#problems-ranked).

## Cost

Per operation on wasm32, with the copy-on-write test hoisted where D8
allows. "Load" is a memory load; locals are free.

| Operation | Built-in List today | One-word hd `List` | One-word `UniqList` | `UniqList` today (3 words) |
| --- | --- | --- | --- | --- |
| `len()` | 1 load | 1 load | 1 load | 0 in a local |
| `xs[i]` read | bounds against a hoisted length, 1 load | same, when D9's facts hold | same | 1 load |
| `push` with room | 2 loads, 2 stores | 2 loads, 2 stores | 2 loads, 2 stores | 1 store in a local |
| copy of the value | 1 load, 1 store | 1 load, 1 store | a move | a move |
| value size | 4 bytes | 4 bytes | 4 bytes | 12 bytes |
| block header | 24 bytes | 16 bytes | 8 bytes | 8 bytes, plus 12 in the value |

## Prior Art

| Library | Shape | Source |
| --- | --- | --- |
| Swift `ContiguousArray` | one counted object, header and elements; `isUniquelyReferenced` before a write | <https://github.com/swiftlang/swift/blob/main/stdlib/public/core/ContiguousArrayBuffer.swift> |
| Swift `ManagedBuffer` | `create(minimumCapacity:makingHeaderWith:)`; `capacity` reads the allocator's real size | <https://developer.apple.com/documentation/swift/managedbuffer> |
| Rust `Vec` | three words; `RawVec` frees, `Vec` drops `len` items | <https://doc.rust-lang.org/src/alloc/vec/mod.rs.html> |
| `thin-vec` | one word; `{len, cap}` header in the block; a static empty header | <https://docs.rs/thin-vec> |
| `triomphe::ThinArc` | one counted word to `HeaderSlice<HeaderWithLength<H>, [T]>` | <https://docs.rs/triomphe> |

## Superseded Shapes

| Shape | Why not |
| --- | --- |
| `List[T] { store: *UniqList[T] }` (#1219 body, 2026-11-03) | a second hop; lanes 46-48 measured 78 ms against 19 ms |
| a separate ManagedBuffer type | D2: `box` takes the unsized head itself |
