# Raw Buffers: Lists Own Their Elements

Accepted 2026-11-03; domain: runtime, ownership pass, `std.rt`,
`std.collections`; being built under #1205.

## The Proposal

The two collection families stay as decided: `List[T]` and `Map` are
`Copy` with copy-on-write and are counted; `UniqList[T]` and `UniqMap`
are `!Copy`, have one owner, and take no count and no copy-on-write
check. What changes is the storage under them:

- `std.rt.memory.Buffer[T]` is raw memory: an address and a capacity.
  It is `!Copy`, has one owner, and dropping it frees the memory and
  nothing else. It knows nothing about elements: no length, no element
  references, no element drops.
- `UniqList[T]` is a buffer plus a length. It moves elements in and out
  through `at(i)`, gives element references, and drops its first `len`
  elements before its buffer frees the memory. `UniqMap` is the same over
  a buffer of slots. `List[T]` is the counted copy-on-write layer over
  the same storage (#1219).
- One invariant holds in every build: **no value of a non-copyable type
  is ever copied**. The ownership pass never chooses a copy for one, and
  a verifier checks it after the pass.

```text
# hypothetical syntax; std.rt.memory
pub struct Buffer[T < ?Copy]:           # raw memory; dropping it frees the room, never an element
    address: usize
    capacity: usize

impl[T < ?Copy] !Copy for Buffer[T]

# std.collections
pub struct UniqList[T < ?Copy]:
    buffer: Buffer[T]
    len: usize

impl[T < ?Copy] !Copy for UniqList[T]

impl[T < ?Copy] Drop for UniqList[T]:   # std.rt.Drop: only std implements it
    fn drop(&mut self) -> void:
        let index: usize = 0
        while index < self.len:
            _ := self.buffer.at(index).read()   # moves the element out; its glue runs
            index += 1
        # then the buffer field's own drop frees the room
```

## Decisions

1. `Buffer` is `!Copy`, owns its memory, and frees it when dropped, as
   Rust's `RawVec`. It has no length and no element operations.
2. A list drops its elements through `std.rt.Drop`, a trait only std may
   implement ([`module.rt.drop.glue`](../../spec/lang/10-modules.md#r-module.rt.drop.glue)).
3. The verifier runs in every build.

## Problems It Solves

**1. Every new path double-frees or leaks.** Today a buffer is a counted
heap object, so a copy of its holder is a retain, and that retain hides
the copy. Removing the count (chat's 52d7a01e5) double-freed wherever the
compiler still copied a holder. Each lane since found the next copy: a
loop loan (#1230), a dynamic call (#1279), a reference view (#1281), a
move while borrowed (#1283), a match through a reference (#1288), a field moved out of a binding
still in use (#1283 again). Under
the proposal a copy of a `!Copy` value is a verifier error at compile
time, so one run over the conformance suite and std lists every site.

**2. `UniqList` pays for counting it was defined without.** Today's
`Buffer` is a counted heap object with a length that drops its own
elements: `List`'s storage under another name. So every by-value use of
a `UniqList` is a count operation, and `item_ref(i)` outside a short
loan counts the buffer through the page table (#1280). With a raw buffer
there is nothing to count, and an element reference is an address, as
in Rust.

**3. The collector special-cases buffers.** #1255 marks a buffer's
header with both color bits (`OWNED`) so a trace enters it as part of
its holder. With no buffer header, the holder's release glue is the
list's `drop`, which releases each element, so the collector traces the
elements through the same glue it already runs for every object.

**4. A header per list.** Each buffer has a 16-byte header (capacity,
length, owner, padding). The proposal keeps capacity and length in the
list value itself, as Rust's `Vec`.

## How It Works

- **Buffer.** `Buffer::new(capacity)` allocates room for `capacity`
  values, `at(i)` is a `Ptr[T]`, and `grow(capacity)` moves the bytes to
  a larger room. Its glue frees the room and drops nothing; it is never
  counted.
- **Drop.** `std.rt.Drop` has one method, `fn drop(&mut self)`. A type
  that implements it gets glue that calls `drop`, then releases its
  fields. Only std may implement it; user code keeps `Disposable` and
  `using` for resources.
- **The rule.** The ownership pass decides each read's mode
  (`ReadMode` in `crates/hd_tir/src/ir.rs`). Today an undecided read
  becomes `Copy` and later passes upgrade it to `Move` or `Borrow`
  (`crates/hd_tir/src/own/insert.rs`, around line 315). Under the
  proposal a read of a `!Copy` value starts as the checker's decision: a
  move at a move site ([`ref.move.sites`](../../spec/lang/13-values-and-references.md#r-ref.move.sites)),
  else a borrow. A use the checker accepted but that needs a copy is a
  compiler bug, reported as an internal error, never emitted.
- **The verifier.** After stage 2, no `Read` of a `!Copy` type has mode
  `Copy`, and no retain names a `!Copy` type. It runs on every body.
- **The collector.** A free during a trace is deferred to the `owned`
  list, which keeps today's free order: a white holder's room is freed
  after `later` is released.
- **The test matrix.** One generated conformance family covers every
  erased call shape, reference kind (`&`, `&mut`, value), receiver
  (`&self`, `&mut self`, `self`) and element kind (`Copy`, counted,
  `!Copy`), each run in checked builds.

## Cost

| Component | Change |
| --- | --- |
| Removed | `BufferBuf`/`BufferDrop` glue, the `OWNED` mark, the 16-byte buffer header, page-table counts of item references, #1280, #1290 |
| Added | `std.rt.Drop` and its glue call; the verifier (one pass over reads); `UniqList`'s length field; the test matrix |
| Compile time | one linear pass per body |
| Runtime | no buffer count operations; one fewer allocation header per list |
| Wasm size | smaller: no buffer glue |
| Web compiler gzip | within noise of today's 1.89 MB, under the 2 MB cap |

## Appendix A. Stress Test

| Case | What happens |
| --- | --- |
| `UniqList[string]` built, popped, dropped | `drop` reads out the remaining strings, each released once, then frees the room |
| a struct holding a `UniqList` passed by value | the struct is `!Copy` through its field, so the pass moves it; a copy is a verifier error |
| `match o:` with `o: &mut Option[UniqList[string]]` (#1288) | the scrutinee is a borrow; today it is `Read copy`, which the verifier rejects |
| a move while `get_ref` is live (#1283) | a checker error, `conflicting-access`; no silent copy |
| `eat(p.a)` then `p.b.len()`, with `p.a` non-copyable | today `Read copy [$0 .a]`: move liveness is per local, not per field (found by #1283). The verifier rejects it, so phase 2 needs per-field liveness and a backend move of a part out of a cell |
| an erased generic method lends `&mut UniqList[T]` to a thunk (#1281) | the view moves the value out and back; no copy |
| a cycle through `UniqList[*Node]` | the holder's glue calls `drop` in trace mode, which hands each `*Node` to `visit`; the room is freed after `later` |
| `UniqListIntoIter` dropped half consumed | unyielded values are dropped, then the room is freed |
| a panic inside an element's glue during `drop` | as today: a panic ends the program, so no partial state is observed |
| `UniqMap` over a buffer of slots | the same `Drop`: it drops the occupied slots, then frees the room |
| `UniqList.push` grows the buffer | `grow` moves the bytes, and the old room is freed once; the buffer is moved or borrowed, never copied |
| user code implements `std.rt.Drop` | `std-only-use`: a runtime module |

## Appendix B. Prior Art

**Rust, `RawVec` and `Vec`.** The raw allocation frees memory only; the
vector drops its elements.

```rust
impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.as_mut_ptr(), self.len)) }
        // RawVec's own Drop then frees the allocation
    }
}
```

hd takes the split as it is. Source: <https://doc.rust-lang.org/src/alloc/vec/mod.rs.html>.

**Swift, `ManagedBuffer`.** A counted heap object with a header and
elements; the subclass's `deinit` destroys the elements.

```swift
final class Storage<T>: ManagedBuffer<Int, T> {
    deinit { _ = withUnsafeMutablePointers { h, e in e.deinitialize(count: h.pointee) } }
}
```

This is today's hd `Buffer`. hd moves away from it because the count
hides copies the compiler should not make. Source:
<https://developer.apple.com/documentation/swift/managedbuffer>.

**Go slices.** A slice header (pointer, length, capacity) is copied
freely; the garbage collector owns the elements. hd has no tracing
collector for acyclic data, so the list must drop them.

## Appendix C. Alternatives Considered

| Alternative | Why not |
| --- | --- |
| keep a counted `Buffer` and fix copies as lanes find them | the count hides each copy; the lanes since #1252 show it never ends |
| today's `Buffer`, counted, with a length and element drops | it is `List`'s storage under another name, and its count is what `UniqList` exists to avoid |
| a copyable `Buffer` freed by an explicit `free` | an owner of memory that copies invites a double free; the verifier could not tell which copy frees |
| a `Drop` trait for user code | resources already have `Disposable` and `using`; automatic user drops are a language feature this does not need |
| a checked-build runtime trap only (#1252) | it fires only on paths a test reaches; the verifier covers every body |

## Appendix D. Today In hd

- `Buffer[T]` is "a counted heap object: a capacity word, a length word,
  an owner word and a padding word, then room for `capacity()` values"
  (`lib/std/rt/memory.hd`; [`module.rt.buffer.type`](../../spec/lang/10-modules.md#r-module.rt.buffer.type),
  [`module.rt.buffer.drop`](../../spec/lang/10-modules.md#r-module.rt.buffer.drop)).
- `UniqList[T]` is one field, `buffer: Buffer[T]`; `UniqMap` holds
  `slots: Buffer[UniqMapSlot[K, V]]`.
- The ownership pass defaults an undecided read to `Copy`
  (`crates/hd_tir/src/own/insert.rs`), and inserts a `Copy` read when a
  consumed value is still needed.
- Checked builds trap any retain of a buffer copy (#1252) and a plain
  release of an owned buffer (#1279); checked conformance passes 5,775 of
  5,775 with both on (main df208fe0a).

## Appendix E. Details

- **Spec.** `module.rt.buffer.type`, `.not-copyable` and `.drop` are
  replaced: raw memory with one owner, freed when dropped, no length.
  New: `std.rt.Drop`, std only. `ref.move.sites` unchanged.
- **Soundness.** Only std reaches `Buffer` and `Drop`. A list's methods
  keep `len` honest, as `set_len` callers do today.
- **Phases**, each about an hour, each with its test:
  1. The verifier, reporting every site on the conformance suite and std;
     the site list goes on the tracking issue. Applied by #1295: it runs
     on every body in every build and is an error.
  2. The read-mode rule for `!Copy` values; the verifier passes. Applied
     by #1296.
  3. `std.rt.Drop` and its glue call; a stage case and a checked case. Applied by #1297 ([Runtime Drop Hook](../../spec/lang/10-modules.md#runtime-drop-hook)).
  4. Raw `Buffer`, `UniqList` with `len`; uniq fixtures in checked
     builds. Applied by #1298 ([Typed Memory](../../spec/lang/10-modules.md#typed-memory)):
     a room keeps an 8-byte head. Its count word was then the buffer's
     count plus one per reference that left a short loan (#1320); see
     phase 6 for what replaced it.
  5. `UniqMap` and the iterators on `Drop`; the collector's deferred
     free; the cycle tests. `UniqMap`'s `Drop` and the deferred free
     applied by #1298; the iterators are #1299.
  6. Remove `OWNED`, `BufferDrop` and the item-reference counts; release
     sizes and the #1205 benchmark against `Map`. `OWNED` and the buffer
     glue removed by #1298. The room's count went with #1336 phase 10
     (#1321): no reference counts a room and `free_room` frees outright;
     its 8 bytes stay only as a reserved header (count 1, descriptor 0)
     that a bytes-to-string transfer takes as the string's, so that
     transfer copies nothing.
  7. The generated test matrix. Applied by #1300.
