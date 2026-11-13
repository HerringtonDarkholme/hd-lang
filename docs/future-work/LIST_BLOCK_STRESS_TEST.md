# List Blocks: Stress Test

Status: design review, 2026-11-12; changes no decision, design record,
spec text, or compiler code. It tests decisions D1 to D10 and open
questions O1 to O6 of [List Blocks](LIST_BLOCK.md) (#1219, #1220),
against [Typed Memory](../../spec/lang/10-modules.md#typed-memory),
[Runtime Drop Hook](../../spec/lang/10-modules.md#runtime-drop-hook),
[Boxes](../../spec/lang/13-values-and-references.md#boxes) and
[Weak runtime support](../../spec/lang/13-values-and-references.md#r-ref.weak.target.whole).

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Open Questions: Options](#open-questions-options)
- [Problems, Ranked](#problems-ranked)
- [Owner Questions, Ranked](#owner-questions-ranked)
- [Parse Log](#parse-log)

## Surface Being Tested

The decided operations, with this report's assumptions where the record
is open. `Slots[T]` (O2 option A), `Slots.at` (O4 option A) and `Own[S]`
(O1 option A) are placeholders, chosen only so the examples can be
written. [Open Questions: Options](#open-questions-options) compares them
with the alternatives.

```text
# std.rt.memory, std-only. "assumed" marks this report's placeholders.

## assumed: the tail marker; zero bytes, last field only (O2 option A)
pub struct Slots[T < ?Copy]:
    pass

impl[T < ?Copy] Slots[T]:
    ## assumed: the place of slot `index` (O4 option A)
    @intrinsic("slots_at")
    pub fn at(&self, index: usize) -> Ptr[T]:
        panic("intrinsic")

## decided D6: the tail-struct form of `box`
@intrinsic("box")
pub fn box[S < ?Copy](head: S, slots: usize) -> *mut S:
    panic("intrinsic")

## decided D6
@intrinsic("regrow")
pub fn regrow[S < ?Copy](r: &mut *S, slots: usize) -> void:
    panic("intrinsic")
```

The stores, the list values and the drop hook (D1, D4, D7):

```text
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

pub struct UniqList[T < ?Copy]:
    block: Own[UniqStore[T]]    # assumed (O1 option A)

impl[T < ?Copy] !Copy for UniqList[T]

impl[T < ?Copy] Drop for UniqStore[T]:
    fn drop(&mut self) -> void:
        let i: usize = 0
        while i < self.len:
            _ := self.items.at(i).read()
            i += 1
```

`unique_mut`, `make_unique` and the loop hoist are as on main
([`module.rt.unique`](../../spec/lang/10-modules.md#r-module.rt.unique),
[`module.rt.make-unique`](../../spec/lang/10-modules.md#r-module.rt.make-unique)).
The block layouts are the record's [The Block](LIST_BLOCK.md#the-block).
Costs are for wasm32, from `crates/hd_wasm/DESIGN.md` §23.4 to §23.7 and
`lib/std/rt/alloc.hd`.

## Method

**Cases.** 19 cases: nine library shapes (Swift `ContiguousArray` and
`ManagedBuffer`, Rust `Vec`, `thin-vec`, `triomphe`, `smallvec`, Go
slices, Zig `ArrayList`, hashbrown), a string builder, and nine std
operations. Library shapes are approximations, written from their public
API and documented internals, not pasted source.

**Verdicts.** *Works*: the design expresses it with the same behavior
and cost class. *Friction*: it works with a workaround, a visible
difference, or a cost the built-in List does not pay. *Breaks*: the
design cannot express it, or an operation gives a different answer.

**Prices.** "Load" and "store" are linear-memory accesses. Wasm locals
are free. A price is per operation, with the copy-on-write test hoisted
wherever D8's rule allows.

**Limits.** Every `text` block was run through
`scripts/hd-conformance.sh parse`. Parsing checks syntax only; no block
is claimed to type-check, and the placeholders do not exist in std.

## Summary

| # | Case | Library | Verdict | At fault |
| --- | --- | --- | --- | --- |
| C1 | `append` on a shared, full array | Swift `ContiguousArray` | breaks | D8 `make_unique` signature; D6 `regrow` when shared |
| C2 | create with a minimum capacity | Swift `ManagedBuffer` | friction | O2 literal; O3 real capacity |
| C3 | drop `len` items, grow by doubling | Rust `Vec`/`RawVec` | works | |
| C4 | an empty list allocates nothing | `thin-vec` | friction | O1 empty block |
| C5 | a counted header plus slice | `triomphe::ThinArc` | works | |
| C6 | inline small storage | `smallvec` | breaks | out of scope |
| C7 | `append` aliasing | Go slices | works | |
| C8 | reserve exactly, shrink to fit | Zig `ArrayList` | friction | O5 shrink |
| C9 | entries and control bytes in one block | hashbrown | friction | O4 typed tail |
| C10 | build bytes, take the string | Go `strings.Builder` | friction | D7 header; one-word `UniqList` |
| C11 | `push` in a loop | std | friction | D8 hoist rule vs `regrow` |
| C12 | sort in place | std | works | |
| C13 | `List` to `UniqList` | std | works | |
| C14 | change while iterating | std | works | |
| C15 | a list of lists | std | works | |
| C16 | a list field behind `&mut self` | std | friction | D9 compiler facts |
| C17 | `weak` of a `UniqList` element | std | breaks | D7 no header |
| C18 | release a `List[string]` | std | breaks | D4 "drops none" |
| C19 | `Map` copy-on-write | #1220 | breaks | D8 `make_unique` signature |

**Conclusions.** One pointer to one block holds up: Swift's
`ContiguousArray`, `thin-vec` and `triomphe` all ship it, and every std
operation translates. Three decided points do not hold as written.
`make_unique` cannot copy an unsized head (C1, C19). `regrow` on a shared
box would release elements twice (C1). "`List[T < Copy]` drops none"
leaks counted items (C18). The headerless uncounted block also lets
`weak` mistake an element for an object (C17). The one-word `UniqList`
moves `len` and `cap` from Wasm locals into memory, a cost that today's
three-word `UniqList` does not pay (C3, C10).

## Cases

### C1. Swift `ContiguousArray.append` on a shared, full array

```swift
public mutating func append(_ newElement: __owned Element) {
  _makeUniqueAndReserveCapacityIfNotUnique()
  let oldCount = _buffer.mutableCount
  _reserveCapacityAssumingUniqueBuffer(oldCount: oldCount)
  _appendElementAssumeUniqueAndCapacity(oldCount, newElement: newElement)
}
// When shared, _createNewBuffer(bufferIsUnique: false,
//   minimumCapacity: count + 1, growForAppend: true): one copy, grown.
```

Source: `stdlib/public/core/Array.swift` in swiftlang/swift.

```text
impl[T] List[T]:
    pub fn push(&mut self, value: T) -> void:
        let s = make_unique(&mut self.store, copy_store)
        if s.len == s.cap:
            need := grown(s.cap)
            regrow(&mut self.store, slots = need)
            s = make_unique(&mut self.store, copy_store)
        s.items.at(s.len).write(value)
        s.len += 1

# make_unique's `copy: fn(&T) -> T` asks for this signature,
# but a tail struct has no by-value form: its slots cannot come along.
fn copy_store[T](s: &ListStore[T]) -> ListStore[T]:
    ListStore::[T] { len: s.len, cap: s.cap }
```

- `make_unique` stores `box(copy(*r))`. Neither `*r` nor `copy`'s result
  can be a tail struct by value, so no `copy` exists. **Breaks** (P1).
- `regrow` on a shared box "moves to a new block and releases the old
  one" (D6). Moving `string` items' bytes without a count, while the old
  block lives on in another list, releases each item twice. **Breaks**
  (P3).
- Even with both fixed, a push to a shared full list copies twice: into
  `cap` slots, then into `2 * cap`. Swift copies once.
- After `regrow`, `s` is stale, so std tests the count again: one load
  and a compare per growth. A `regrow` that returns `&mut S` removes it.

### C2. Swift `ManagedBuffer.create`

```swift
final class Storage: ManagedBuffer<(count: Int, cap: Int), Int> {}
let b = Storage.create(minimumCapacity: 10) { buf in (count: 0, cap: buf.capacity) }
// buf.capacity comes from malloc_size: 10 or more
```

```text
impl[T] List[T]:
    pub fn with_capacity(n: usize) -> List[T]:
        List::[T] { store: box(ListStore::[T] { len: 0, cap: n }, slots = n) }
```

- The literal leaves out `items`. Today that is `missing-required-field`
  ([`data.literal.required`](../../spec/lang/08-data-and-enums.md#r-data.literal.required)), so a
  tail struct needs a literal rule (P9).
- The allocator rounds the block up to its size class. For 11 `i64`
  slots (104 bytes with the headers) it gives 112, room for 12, and `cap`
  records 11. Swift
  reads the real room back; hd cannot (P8). **Friction.**

### C3. Rust `Vec` and `RawVec`: drop and grow

```rust
impl<T, A: Allocator> Drop for Vec<T, A> {
    fn drop(&mut self) {
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.as_mut_ptr(), self.len)) }
    }
}
// RawVec::grow_amortized: cap = max(cap * 2, len + additional), then realloc
```

```text
impl[T < ?Copy] UniqList[T]:
    pub fn push(&mut self, value: T) -> void:
        let s = self.block.get_mut()
        if s.len == s.cap:
            need := grown(s.cap)
            regrow(&mut self.block, slots = need)
            s = self.block.get_mut()
        s.items.at(s.len).write(value)
        s.len += 1
```

- The drop hook in the surface is `Vec`'s `drop_in_place` of `len`
  items; the language frees the block after it, as `RawVec` does.
  **Works.**
- `Vec` keeps `len` and `cap` in the value, so a push in a loop runs in
  registers. The one-word `UniqList` pays 2 loads and 1 extra store per
  push. Today's three-word `UniqList` in a local pays none of them (P6).

### C4. `thin-vec`: an empty list allocates nothing

```rust
pub fn new() -> ThinVec<T> {
    unsafe { ThinVec { ptr: NonNull::new_unchecked(&EMPTY_HEADER as *const Header as *mut Header), boo: PhantomData } }
}
```

```text
impl[T < ?Copy] UniqList[T]:
    pub fn new() -> UniqList[T]:
        # one 8-byte allocation per empty list, unless a static head may stand in
        UniqList::[T] { block: own(UniqStore::[T] { len: 0, cap: 0 }, slots = 0) }
```

- The counted `List`'s `[]` keeps today's static block with count 0, so
  it allocates nothing, and `make_unique` copies it on the first write.
- Nothing decided lets an uncounted box refer to a static head.
  `free_block` already ignores a static page, so the runtime would
  accept one; the rule is missing (P7). **Friction.**
- First push on `[]`: `make_unique` copies the empty block (one
  allocation), then `regrow` moves it (a second). `thin-vec` and Swift
  make one.

### C5. `triomphe::ThinArc`: a counted header plus slice

```rust
let a: ThinArc<u32, u8> = ThinArc::from_header_and_iter(7, b"abc".iter().copied());
let b = a.clone(); // one count increment; the length lives in the header
```

```text
fn from_items[T](items: &UniqList[T]) -> List[T]:
    n := items.len()
    store := box(ListStore::[T] { len: n, cap: n }, slots = n)
    let i: usize = 0
    while i < n:
        store.items.at(i).write(items[i])
        i += 1
    List::[T] { store: store }
```

**Works.** One allocation, the length in the head, a one-word value.
triomphe's `UniqueArc` keeps its count word, so `shareable()` costs
nothing; D7 drops the word and makes that conversion copy (C13).

### C6. `smallvec`: inline storage

```rust
let mut v: SmallVec<[u32; 4]> = SmallVec::new();
v.push(1); // no allocation until the fifth item
```

```text
fn small() -> List[u32]:
    let mut v: List[u32] = []
    v.push(1)
    v
```

**Breaks**, by design: a one-word value has no room for items, and the
first push allocates. No decision is at fault; inline storage would be a
separate type.

### C7. Go slices: `append` aliasing

```go
s := make([]int, 0, 4)
t := append(s, 1)
u := append(s, 2) // the same array: t[0] is now 2
```

```text
fn go_append() -> void:
    let mut s: List[i64] = []
    s.reserve(4)
    let mut t = s
    t.push(1)
    let mut u = s
    u.push(2)
```

**Works.** `t` and `u` each copy the shared block on their first push, so
`t[0]` stays 1. A Go program that relies on the shared array must hold
`*mut List[i64]`; this is today's List behavior too.

### C8. Zig `ArrayList`: reserve exactly, shrink

```zig
var list = std.ArrayList(u8).init(allocator);
defer list.deinit();
try list.ensureTotalCapacityPrecise(100);
list.appendAssumeCapacity('a');
list.shrinkAndFree(list.items.len);
```

```text
fn zig_like() -> void:
    let mut list = UniqList::[u8].with_capacity(100)
    list.push(97)
    list.shrink_to_fit()
```

**Friction.** D6 defines `regrow` for growth only. `shrink_to_fit` and
#1317's `truncate` row need a rule for fewer slots: no effect, as
`Buffer.grow` today, or a move to a smaller class (O5).

### C9. hashbrown: entries and control bytes in one block

```rust
// RawTableInner: one allocation, buckets below the control bytes:
// [ T(n-1) ... T(1) T(0) | ctrl(0) ... ctrl(n-1) | mirror of the first group ]
let (layout, ctrl_offset) = table_layout.calculate_layout_for(buckets)?;
```

```text
struct MapStore[K < Eq & Hash, V]:
    size: usize
    used: usize
    entry_capacity: usize
    seed: u64
    entries: Slots[MapSlot[K, V]]

impl[K < Eq & Hash, V] MapStore[K, V]:
    # the probe index lives past the entries, in slots counted in entry units
    fn index_at(&self, bucket: usize) -> Ptr[u32]:
        Ptr::[u32] { address: self.entries.at(self.entry_capacity).address }.offset(bucket)
```

**Friction.** A typed tail of one `T` holds the index only by counting
it in entry-sized slots, as `UniqMap` does today. It works, but the slot
count stops meaning "entries", and `Slots.at` must allow an address past
the entries (O4). A map grows by rehashing into a new box, so it never
calls `regrow`.

### C10. A string builder: build bytes, take the string

```go
var b strings.Builder
b.Grow(64)
for i := 0; i < 3; i++ {
    b.WriteByte('a')
}
s := b.String() // no copy: the string takes the builder's bytes
```

```text
fn build() -> string:
    let mut b = UniqList::[u8].with_capacity(64)
    let i: usize = 0
    while i < 3:
        b.push_byte(97)
        i += 1
    b.take_text()
```

- Today the transfer takes no copy: a `Buffer` room keeps a reserved
  8-byte header, which the string takes as its own
  (`string_from_buffer`, `lib/std/rt/memory.hd` `room`).
- With no header (D7), the block starts with `len` and `cap`. On wasm32
  those are exactly 8 bytes, so the transfer can write the count and
  descriptor over them and still copy nothing. The coincidence needs a
  rule, or the transfer copies (P13).
- Each `push_byte` loads `len` and `cap` and stores `len`, where today's
  `UniqList` keeps them in locals (P6). **Friction.**

### C11. `push` in a loop

```rust
let mut v = Vec::new();
for i in 0..n { v.push(i as i64); }
```

```text
fn build(n: usize) -> List[i64]:
    let mut xs: List[i64] = []
    let i: usize = 0
    while i < n:
        xs.push(i64(i))
        i += 1
    xs
```

- After inlining, each iteration runs `make_unique` on `xs.store`, and the
  cold path runs `regrow` on it.
- [`module.rt.make-unique.copy`](../../spec/lang/10-modules.md#r-module.rt.make-unique.copy)
  hoists only when "nothing in the loop shares or replaces that place's
  box". `regrow` replaces it, so as written the test stays in the loop: 1
  load and 1 compare per push. The built-in List hoists through its own
  push today (`hoist.rs`, the `pushes` field). **Friction** (P2).
- Price with P2 fixed: 2 loads, 2 stores per push, as the built-in List.

### C12. Sort in place

```text
impl[T < Ord] List[T]:
    pub fn sort(&mut self) -> void:
        s := make_unique(&mut self.store, copy_store)
        drift_sort(s.items.at(0), s.len)
```

**Works** (once P1 is settled). One count test, then the kernel moves
elements between slots and scratch, as it does for `UniqList` today.

### C13. `List` to `UniqList`

```rust
let v: Vec<String> = rc_slice.to_vec(); // Rc<[String]> to Vec: allocate and clone
```

```text
fn to_uniq(xs: List[string]) -> UniqList[string]:
    UniqList::[string].from_list(xs)
```

**Works**, as D7 decided: one allocation and a copy with one retain per
item. When `xs` holds the only count, a consuming conversion could move
the items and skip `n` retains and `n` releases. It still copies bytes:
the uncounted block has no header to keep.

### C14. A change while iterating

```text
fn double_positive(mut xs: List[i64]) -> List[i64]:
    for x in xs:
        if x > 0:
            xs.push(x)
    xs
```

**Works.** The loop's snapshot holds a count, so the first push copies
once ([`flow.for.snapshot.cost`](../../spec/lang/06-control-flow.md#r-flow.for.snapshot.cost)).
The push is conditional, so nothing hoists, as today.

### C15. A list of lists

```text
fn fill_row(mut grid: List[List[i64]], i: usize, n: usize) -> List[List[i64]]:
    let j: usize = 0
    while j < n:
        grid[i].push(i64(j))
        j += 1
    grid
```

**Works.** The outer `make_unique` gives `&mut ListStore`, slot `i` gives
`&mut List`, and the inner push may `regrow`, which writes slot `i`. The
inner test stays in the loop: `hoist.rs` takes a place with fields only,
no element step, as for the built-in List today. A one-word element makes
the outer block 4 bytes per row, against 12 for three words.

### C16. A list field behind `&mut self`

```text
struct Log:
    lines: List[string]

impl Log:
    fn add_all(&mut self, more: &List[string]) -> void:
        let i: usize = 0
        while i < more.len():
            self.lines.push(more[i])
            i += 1
```

**Friction.** The place `[*self .lines .store]` has one dereference and
fields, so `hoist.rs` rule 3 applies as for the built-in List. Hoisting
the `len` and `cap` loads needs D9's two named gaps: a `make_unique`
result as a root, and a list in a frame slot.

### C17. `weak` of a `UniqList` element

```text
use std.weak.weak

struct Node:
    id: i64

fn watch(nodes: &UniqList[Node]) -> void:
    w := weak(&nodes[0])
```

- [`ref.weak.target.refuse`](../../spec/lang/13-values-and-references.md#r-ref.weak.target.refuse)
  says an element is refused. The runtime test (`rt:weak_new`) accepts an
  address that is a block's start plus 8, then reads the word at
  block + 4 as the descriptor.
- A headerless block puts slot 0 at block + 8 and `cap` at block + 4.
  When `cap` equals `Node`'s glue slot plus one, the test succeeds, and
  binding the entry sets bit 15 of `cap`. The list then believes it has
  32,768 more slots. **Breaks** (P5), silently.
- A `List` element is safe: its slot 0 is at block + 16.
- `ref_identity` is the place's address (`emit/refs.rs`), so it works for
  any slot, and changes when `regrow` or a copy moves the block, as for
  the built-in List.

### C18. Releasing a `List[string]`

```text
fn names() -> void:
    xs := ["ada", "lin"]
    ys := xs
    println(ys[0])

impl[T] Drop for ListStore[T]:
    fn drop(&mut self) -> void:
        let i: usize = 0
        while i < self.len:
            _ := self.items.at(i).read()
            i += 1
```

**Breaks** as D4 is worded. `Copy` includes every counted type:
`string`, `*T`, `List[T]` ([`ref.copy.structure`](../../spec/lang/13-values-and-references.md#r-ref.copy.structure)).
The language treats slots as empty, so if the hook "drops none", no one
releases the two strings: both leak. The hook above must run for every
`T`; for a `T` with no release glue (`i64`), its loop must compile to
nothing (P4). The same holds for the copy-on-write copy, which must be
one `memory.copy` for such a `T`.

### C19. `Map` copy-on-write (#1220)

```text
impl[K < Eq & Hash, V] Map[K, V]:
    pub fn put(&mut self, key: K, value: V) -> void:
        t := make_unique(&mut self.table, copy_table)
        t.insert_unique(key, value)
```

**Breaks** for P1's reason: `copy_table` would return a `MapStore` by
value. Once P1 is settled, a copy copies entries with one retain each and
the index with one `memory.copy`.

## Comparison With Other Languages

| Concern | hd as decided | Swift | Rust | thin-vec / triomphe |
| --- | --- | --- | --- | --- |
| value size | 1 word | 1 word | `Vec`: 3 words | 1 word |
| count | in the block (`List`); none (`UniqList`) | in the object | none (`Vec`); `Arc` in the block | `ThinArc` in the block |
| empty | static block (`List`); open (`UniqList`) | one static empty storage | dangling pointer, no allocation | static `EMPTY_HEADER` |
| shared write | `make_unique` then `regrow`: 2 copies | one copy into a grown buffer | `Arc::make_mut`: one clone | `make_mut` |
| real room | not readable (P8) | `malloc_size` | `Allocator` returns the real size | none |
| shrink | open (O5) | `reserveCapacity` never shrinks | `shrink_to_fit` reallocs | `shrink_to_fit` |
| unique to shared | copy (D7) | n/a | `Arc::from(vec)`: copy | `UniqueArc::shareable`: free |

Sources: Swift `stdlib/public/core/ContiguousArrayBuffer.swift` and
<https://developer.apple.com/documentation/swift/managedbuffer>; Rust
<https://doc.rust-lang.org/std/vec/struct.Vec.html> and
<https://doc.rust-lang.org/std/sync/struct.Arc.html#method.make_mut>;
<https://docs.rs/thin-vec>; <https://docs.rs/triomphe>.

## Open Questions: Options

Each question gives options in parsed hd, a priced comparison, and a
**Recommendation**, kept apart from the facts.

### O1. The uncounted box

**A. `Own[S]`, a std-only struct the compiler knows by path**, as it
knows `Buffer` and `Ptr`:

```text
pub struct Own[S < ?Copy]:
    address: usize

impl[S < ?Copy] !Copy for Own[S]

impl[S < ?Copy] Own[S]:
    @intrinsic("own_ref")
    pub fn get['a](&'a self) -> &'a S:
        panic("intrinsic")

    @intrinsic("own_mut")
    pub fn get_mut['a](&'a mut self) -> &'a mut S:
        panic("intrinsic")

@intrinsic("own")
pub fn own[S < ?Copy](head: S, slots: usize) -> Own[S]:
    panic("intrinsic")
```

**B. A plain std struct over raw memory**, as `Buffer` is, with a new
`drop_in_place` intrinsic:

```text
pub struct Own[S < ?Copy]:
    at: Ptr[S]

impl[S < ?Copy] !Copy for Own[S]

impl[S < ?Copy] Drop for Own[S]:
    fn drop(&mut self) -> void:
        drop_in_place(self.at)
        free_block(self.at.address)
```

**C. A language pointer type**, `^S`:

```text
pub struct UniqList[T < ?Copy]:
    block: ^UniqStore[T]   # hypothetical syntax
```

**D. `box` with a flag:**

```text
fn make[T](n: usize) -> UniqList[T]:
    b := box(UniqStore::[T] { len: 0, cap: n }, slots = n, counted = false)
    UniqList::[T] { block: b }
```

| | A `Own[S]` known by path | B std over `Ptr` | C `^S` | D `box` flag |
| --- | --- | --- | --- | --- |
| deref | 0: the value is the address | 0 | 0 | — |
| head read | 1 load, a typed place | 1 load through `as_ref` | 1 load | — |
| free | glue: hook, then `free_block` (1 page-table load) | std `Drop`: same work | glue | — |
| compiler | one more known path; the counted glue minus the count | `drop_in_place`; nothing else | grammar, checker, ownership, lowering | the result type depends on a value: not an ordinary declaration ([`ref.box.declaration`](../../spec/lang/13-values-and-references.md#r-ref.box.declaration)) |
| soundness | `!Copy`: one owner, checked by the verifier | same; the drop order is std's to get right | a new pointer kind to keep sound everywhere | — |
| D4 fit | the language frees, as D4 says | std frees, against D4 | language | — |

C's `^` was parked as a mode in #163, and `^S` would offer users a
`Box<T>` no one has asked for.

**Recommendation:** A. It reuses the known-by-path pattern, keeps D4's
"the language frees", and gives the backend a typed place for D9's
header facts at no runtime cost.

### O2. Declaring the tail

**A. A last field of the std-only type `Slots[T]`** (the surface above).

**B. `@tail` on an ordinary field:**

```text
struct ListStore[T]:
    len: usize
    cap: usize
    @tail
    items: T
```

**C. A slice type:**

```text
struct ListStore[T]:
    len: usize
    cap: usize
    items: [T]   # hypothetical syntax
```

**D. A struct annotation, with no field:**

```text
@tail(T)
struct ListStore[T]:
    len: usize
    cap: usize
```

| | A `Slots[T]` | B `@tail items: T` | C `[T]` | D `@tail(T)` |
| --- | --- | --- | --- | --- |
| new syntax | none | none | a type form; the parser rejects `[` today | none |
| std-only by | `module.rt.std-only` on the path | an annotation check | a check on the type form | an annotation check |
| field type tells the truth | yes: slots of `T` | no: says one `T` | yes | no field to read |
| slot access | a method on the field (O4 A) | a new intrinsic | a place or an intrinsic | an intrinsic on the struct |

All four need the same two rules. A tail struct literal leaves the tail
out, and is valid only as `box`'s or `own`'s argument (P9). A tail
struct is never a value: no local, parameter, field or element holds one.

**Recommendation:** A. It adds no syntax and no annotation, and
`std-only` falls out of the module rule that already exists.

### O3. Where the slot count lives for freeing

**A. The allocator.** `free_block` already finds the size class in the
page table (`lib/std/rt/alloc.hd`), and the counted box's descriptor
carries it. The language needs no slot count after allocation. A
std-only query can return the real room:

```text
@intrinsic("slots_of")
fn slots_of[S < ?Copy](r: &S) -> usize:
    panic("intrinsic")
```

**B. The struct's `cap` field, named to the compiler:**

```text
struct ListStore[T]:
    len: usize
    @slots
    cap: usize
    items: Slots[T]
```

**C. A hidden word in the block**, before the head.

| | A allocator | B `@slots` field | C hidden word |
| --- | --- | --- | --- |
| bytes per block | 0 | 0 | 4, or 8 with an 8-aligned tail |
| free | counted: class from the descriptor, 0 extra loads; uncounted: 1 page-table load | 1 load of `cap`, then the class | 1 load |
| `regrow` knows the old size | `block_bytes`: 1 page-table load | 1 load | 1 load |
| compiler | none | reads a std field it must trust | writes and reads the word |
| real room for std (P8) | `slots_of`: 1 load | no | no |

**Recommendation:** A, with `slots_of`. It costs nothing that freeing
does not already pay, and it settles P8.

### O4. How std reaches slot `i`

**A. A method on the tail field**, `items.at(i) -> Ptr[T]` (the surface).

**B. A free intrinsic over the struct:**

```text
@intrinsic("tail_slot")
fn slot[S < ?Copy, T < ?Copy](r: &S, index: usize) -> Ptr[T]:
    panic("intrinsic")
```

**C. The tail's address in bytes:**

```text
@intrinsic("tail_address")
fn tail_address[S < ?Copy](r: &S) -> usize:
    panic("intrinsic")

fn at[T](s: &ListStore[T], i: usize) -> Ptr[T]:
    Ptr::[T] { address: tail_address(s) }.offset(i)
```

**D. An element place:**

```text
fn get[T](s: &ListStore[T], i: usize) -> T:
    s.items[i]
```

| | A `items.at(i)` | B `slot[S, T]` | C `tail_address` | D place |
| --- | --- | --- | --- | --- |
| per access | 1 shift, 1 add | same | same | same |
| `T` checked against the tail | yes | no: any `T` | no | yes |
| backend knows a slot store misses the head | yes: a typed room | yes | only from a `Ptr` | yes |
| hashbrown's index (C9) | `at(cap).address` | same | natural | no |
| compiler | one intrinsic | one intrinsic | one intrinsic | places over empty slots, against D4 |

**Recommendation:** A, plus an `address()` method on `Slots` for byte
tables such as the map index.

### O5. `regrow` on an uncounted box

**A. The same intrinsic, a second signature**, as `string_from_bytes`
has three (`crates/hd_resolve/src/intrinsics.rs`):

```text
@intrinsic("regrow")
pub fn regrow_own['a, S < ?Copy](r: &'a mut Own[S], slots: usize) -> &'a mut S:
    panic("intrinsic")
```

**B. A method on `Own`, with the slots to keep:**

```text
impl[S < ?Copy] Own[S]:
    @intrinsic("own_regrow")
    pub fn regrow['a](&'a mut self, slots: usize, keep: usize) -> &'a mut S:
        panic("intrinsic")
```

| | A one `regrow` | B `Own.regrow(slots, keep)` |
| --- | --- | --- |
| in place | 1 page-table load and a compare; a large run also checks the next pages | same |
| move | `alloc`, `memory.copy` of the old block, `free_block` | same, copying the head and `keep` slots only |
| count test | none | none |
| result | `&mut S`: no second lookup (C1, C3) | same |
| names | one, as D6 says | two |

Both need a rule for fewer slots (C8): no effect, as `Buffer.grow`
today, or a move to a smaller class. Copying the whole old block wastes
nothing in a push, since `len == cap` there; `reserve` on a half-full
list copies twice what it needs.

**Recommendation:** A, returning `&'a mut S`, with fewer slots having no
effect; shrinking can be a later operation.

### O6. Weak references and `ref_identity` on a tail block

`ref_identity` needs nothing: it is the address of the place, as for
today's list elements. A counted tail block is never a user-nameable
weak target, and a `List` slot sits at block + 16, so `weak` refuses it.
The uncounted block is the problem (C17). Options:

| | A page-table bit | B a zero word at block + 4 | C class bits in `weak_new` |
| --- | --- | --- | --- |
| how | uncounted blocks come from pages marked in the page table; `weak_new` refuses them | the head starts `[len, 0, cap]` | `weak_new` also requires the descriptor's class bits to match the page's class |
| bytes per block | 0 | 8 with an 8-aligned tail; D7's saving gone | 0 |
| cost | separate page lists per class: up to 20 partly used 64 KiB pages | none | 1 load and a compare per `weak` |
| sound | yes | yes | for small blocks; a large run needs its own class value |

**Recommendation:** A if D7's 8 bytes matter more than a few resident
pages; else B. C is cheapest but rests on a bit-pattern argument.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | P1 | `make_unique` cannot copy an unsized head | breaks every write | C1, C12, C19 |
| 2 | P4 | "`List[T < Copy]` drops none" leaks counted items | silent leak | C18 |
| 3 | P3 | `regrow` on a shared counted box releases twice | silent double free | C1, C4 |
| 4 | P5 | `weak` mistakes a headerless slot for an object | silent corruption | C17 |
| 5 | P2 | the hoist rule excludes a loop that grows the list | the 5% probe | C11, C16 |
| 6 | P6 | one-word `UniqList` moves `len`/`cap` into memory | slower push loops | C3, C10 |
| 7 | P9 | tail struct literals and by-value use have no rule | spec gap | C2, all |
| 8 | P7 | no static empty block for `UniqList` | one allocation per empty list | C4, C11 |
| 9 | P8 | std cannot read the room it got | lost slack | C2, C8 |
| 10 | P13 | the string transfer relies on an 8-byte head | a copy, or a hidden rule | C10 |

### P1. `make_unique` cannot copy an unsized head

**Effect.** `make_unique(r, copy: fn(&T) -> T)` stores `box(copy(*r))`.
A tail struct has no by-value form, so no `List`, `UniqList`-to-`List`
or `Map` write can call it (C1, C12, C19).

**Candidates.**
1. Should `copy` return the new box, `fn(&S) -> *mut S`, so std allocates
   it with `box(head, slots = n)`?
2. Should `make_unique` allocate a block with the same slots and pass it,
   `fn(&S, *mut S) -> void`?
3. Should P3's combined operation replace this form for tail structs?

```text
pub fn make_unique_tail['a, S < ?Copy](r: &'a mut *S, copy: fn(&S) -> *mut S) -> &'a mut S:
    shared := match unique_mut(r):
        .Some(_) => false
        .None => true
    if shared:
        *r = copy(*r)
    match unique_mut(r):
        .Some(u) => u
        .None => panic("make_unique_tail found a new box shared")
```

### P4. "`List[T < Copy]` drops none"

**Effect.** `Copy` includes `string`, `*T` and `List[T]`. A released
`List[string]` would leak each string, silently (C18).

**Candidates.**
1. Should the hook drop `len` items for every `T`, with the loop compiling
   to nothing when `T` has no release glue?
2. Should std get a std-only `needs_drop[T]()`, as Rust's
   `mem::needs_drop`, so the hook and the copy pick a `memory.copy` path
   by a constant?

### P3. `regrow` on a shared counted box

**Effect.** D6 moves a shared block's bytes and releases the old block.
Its other holders keep the old items, now also in the new block with no
retain: each is released twice (C1). Even avoided by calling
`make_unique` first, a push to a shared full list copies twice (C1, C4).

**Candidates.**
1. Should `regrow` require the only count, and panic or be undefined
   otherwise, with std always calling `make_unique` first?
2. Should one operation make a box unique and big enough, copying once
   into `slots` when shared, as Swift's `_createNewBuffer`?
3. Should `regrow` take a copy function and copy when shared?

```text
@intrinsic("make_room")
pub fn make_room['a, S < ?Copy](r: &'a mut *S, slots: usize, copy: fn(&S, *mut S) -> void) -> &'a mut S:
    panic("intrinsic")
```

### P5. `weak` and a headerless block

**Effect.** `weak(&nodes[0])` on a `UniqList` can pass `rt:weak_new`'s
whole-object test and set a bit in `cap` (C17). Checked builds' leak
report also reads `len` as a count. See O6 under [Open Questions: Options](#open-questions-options).

**Candidates.**
1. Should uncounted blocks come from pages the page table marks?
2. Should the uncounted head keep a zero word at block + 4?
3. Should `weak_new` also check the descriptor's class bits?

### P2. The hoist rule and a growing loop

**Effect.** [`module.rt.make-unique.copy`](../../spec/lang/10-modules.md#r-module.rt.make-unique.copy)
hoists only when nothing "replaces that place's box". `regrow` does, so a
push loop keeps its count test: 1 load and 1 compare per push (C11, C16).

**Candidates.**
1. Should the rule say a `regrow` (or P3's operation) keeps the place
   unique, since its result is the old box or a new one?
2. Should `regrow` be known to `hoist.rs` as the built-in push is today?

### P6. One-word `UniqList` in a hot loop

**Effect.** Today's `UniqList` is `{address, capacity, len}`; in a local,
`len` and `cap` are Wasm locals. One word moves them into the block: 2
loads and 1 store more per push (C3, C10). The 5% probe covers `List`
only.

**Candidates.**
1. Should D9's header facts also keep an uncounted block's `len` and
   `cap` in locals over a loop that owns it, written back at its exits?
2. Should D3's three-word fallback be open to `UniqList` too, measured on
   its own probe?
3. Should the cost be accepted, with a `UniqList` push loop and a string
   builder added to the probe?

### P9. Tail struct literals and by-value use

**Effect.** `ListStore::[T] { len: 0, cap: n }` omits `items`, an error
today (C2). Nothing yet says a tail struct cannot be a local or a field.

**Candidates.**
1. Should a tail struct literal omit its tail and be valid only as
   `box`'s or `own`'s argument, with any other by-value use an error?
2. Should the tail be a zero-size value written in the literal, with
   only std trusted not to copy the head out?

### P7. An empty `UniqList`

**Effect.** `UniqList::new()` allocates 8 bytes, where `thin-vec` and the
counted `[]` use a static block (C4). The first push on `[]` allocates
twice (C11).

**Candidates.**
1. Should an uncounted box be allowed to refer to one static empty head,
   which `free_block` already leaves alone and `regrow` always moves?
2. Should P3's combined operation give the first push one allocation?

### P8. The real room

**Effect.** For 11 `i64` slots the block is 112 bytes, room for 12, but
`cap` says 11 (C2). In-place `regrow` then helps only blocks over 2 KiB.

**Candidates.**
1. Should a std-only `slots_of(r)` return the room the block has?
2. Should `box` round `slots` up to the class and report it?

### P13. The string transfer and the head size

**Effect.** `take_text` copies nothing today because a room keeps an
8-byte header. A headerless `UniqList[u8]` keeps the transfer free only
because `len` and `cap` happen to fill 8 bytes on wasm32 (C10).

**Candidates.**
1. Should the record fix the `UniqList[u8]` head at exactly 8 bytes, so
   the transfer writes a string header over it?
2. Should the transfer copy, accepting one `memory.copy` per string built?

## Owner Questions, Ranked

One question per open item, highest first. Each recommendation is the
report's, labeled, and not a decision.

1. **P1.** How does `make_unique` copy a tail struct? **Recommendation:**
   `copy: fn(&S) -> *mut S`, or fold it into question 3's operation.
2. **P4.** Does `List`'s hook drop `len` items for every `T`?
   **Recommendation:** yes; reword D4, and let a std-only `needs_drop[T]()`
   pick `memory.copy` and an empty drop for plain `T`.
3. **P3.** Is `regrow` on a shared box allowed? **Recommendation:** no;
   add one `make_room(r, slots, copy)` that copies once when shared, as
   Swift does.
4. **O1.** What is the uncounted box? **Recommendation:** `Own[S]`, a
   std-only struct known by path, made by `own(head, slots = n)`.
5. **P2.** Does a `regrow` in a loop block the `make_unique` hoist?
   **Recommendation:** no; the rule should say `regrow` keeps the place
   unique.
6. **O6 / P5.** How does `weak` refuse a headerless slot?
   **Recommendation:** a page-table mark for uncounted blocks; a zero
   word if the pages cost more than D7's 8 bytes save.
7. **O2 / P9.** How does std declare a tail? **Recommendation:** a last
   field of type `Slots[T]`; its literal omits it, valid only in `box` or
   `own`.
8. **O4.** How does std reach slot `i`? **Recommendation:** `items.at(i)
   -> Ptr[T]`, plus `items.address()` for byte tables.
9. **O5.** What does `regrow` do on an uncounted box? **Recommendation:**
   the same `regrow`, returning `&mut S`; fewer slots have no effect.
10. **O3 / P8.** Where does the slot count live? **Recommendation:** in
    the allocator only, with `slots_of(r)` to read the real room.
11. **P6.** Is the one-word `UniqList`'s push cost accepted?
    **Recommendation:** measure a `UniqList` push loop and a string
    builder against today's three words before lane 3.
12. **P7.** May an uncounted box refer to a static empty head?
    **Recommendation:** yes.
13. **P13.** Is the zero-copy string transfer kept? **Recommendation:**
    yes, by fixing the `UniqList[u8]` head at 8 bytes.

## Parse Log

Every `text` block, in order, through `scripts/hd-conformance.sh parse`.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Surface: std.rt.memory | parse |
| 2 | Surface: stores | parse |
| 3 | C1 | parse |
| 4 | C2 | parse |
| 5 | C3 | parse |
| 6 | C4 | parse |
| 7 | C5 | parse |
| 8 | C6 | parse |
| 9 | C7 | parse |
| 10 | C8 | parse |
| 11 | C9 | parse |
| 12 | C10 | parse |
| 13 | C11 | parse |
| 14 | C12 | parse |
| 15 | C13 | parse |
| 16 | C14 | parse |
| 17 | C15 | parse |
| 18 | C16 | parse |
| 19 | C17 | parse |
| 20 | C18 | parse |
| 21 | C19 | parse |
| 22 | O1 A | parse |
| 23 | O1 B | parse |
| 24 | O1 C | rejected: `expected a type, found '^'`; hypothetical syntax |
| 25 | O1 D | parse |
| 26 | O2 B | parse |
| 27 | O2 C | rejected: `expected a type, found '['`; hypothetical syntax |
| 28 | O2 D | parse |
| 29 | O3 A | parse |
| 30 | O3 B | parse |
| 31 | O4 B | parse |
| 32 | O4 C | parse |
| 33 | O4 D | parse |
| 34 | O5 A | parse |
| 35 | O5 B | parse |
| 36 | P1 | parse |
| 37 | P3 | parse |

The two rejections are the hypothetical type forms, which no chapter
specifies. The record's two blocks
([LIST_BLOCK.md](LIST_BLOCK.md#the-block)) also parse.
