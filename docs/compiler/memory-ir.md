# A Memory IR For The Linear Backend

Status: proposal for issue #667. Nothing here is implemented or approved.

## The Problem

hd values behave like Go's. A struct is bytes at an address, a field is
the address plus a constant offset, a reference is a pointer, and
copying a struct copies its bytes. The backend already lays values out
that way (crates/hd_wasm_linear/DESIGN.md §23.3). The IR in front of it
does not think that way.

TIR is the checker's output: generic, typed, structured, one body per
item. The emitter reads it per instance and decides everything about
memory itself. TIR treats values like a dynamic language does:

- **A field write rebuilds the value.** `body.x = 1.0` on a local is
  `LocalGet body; CopyData [.x = 1.0]; LocalSet body`. Through `&mut Body`
  it is a read through the reference, a `CopyData`, and a whole-value
  write back.
- **A reference is a pair of closures.** `&mut T` is a getter and a
  setter (`Field .0 : fn() -> T`, `CallValue`, `RefRead`). The driver
  strips the closures for this backend, and the emitter rebuilds addresses
  from the `RefPlace` record beside them.
- **Collections are opaque objects.** 24 intrinsics (`ListLen`,
  `ListIndex`, `ListSet`, `ListIndexMut`, `ListFieldSet`, `ListPin`,
  `MapGet`, `MapSlots`, `StrConcat`, `ValueCopy`...) each have their own
  emitter code and their own counting rule. Nothing between the checker
  and the bytes can look inside them.
- **Mutation goes through the value, not the place.** `push` takes the
  list as an operand, so the change must reach every holder of that
  value. That is the only reason a list is a header over a buffer: two
  objects per list where one would do (DESIGN.md §23.7 says so).
- **Ownership is invisible.** Retains and releases are decided while
  bytes are written (`emit/count.rs`, `emit/live.rs`, the loans of
  §23.31). No pass can see a count, so none can remove one. Every borrow
  rule is a special case inside the emitter, and the bad-code catalog
  (#656) is mostly counts that a pass over explicit counts would delete.
- **Three `for` lowerings and dead tags.** Lists use a snapshot and an
  index, maps use slot intrinsics, everything else uses the `Iterable`
  cursor. `ForRange`, `ForList` and `ForMap` are declared, matched and
  never produced.

The emitter is about 28,000 lines because it is a type-directed lowering,
an ownership analysis, an optimizer and a Wasm writer at once.

## What Others Do

- **Go** (cmd/compile SSA, go/ssa). Places are addresses:
  - `FieldAddr` and `IndexAddr` in go/ssa; `OffPtr` and `PtrIndex` in the
    compiler;
  - then `Load`, `Store` and `Move` of a typed size.
  
  A slice is `(ptr, len, cap)`, and `append` is a runtime call whose
  result is stored back to the place. A local whose address is never
  taken stays in SSA values, split into its fields. A taken one gets a
  slot. Maps are runtime library code. Bounds checks are explicit
  (`IsInBounds`) and a pass removes the provable ones.
- **Swift SIL.** Places are addresses too (`struct_element_addr`,
  `index_addr`, `load`, `store`). Ownership SSA makes every count
  explicit: `copy_value`, `destroy_value`, `load [copy]` versus
  `load_borrow`, `begin_borrow`. Copy-on-write is `is_unique` followed by
  a copy when the answer is no. Its optimizer removes counts, as a pass
  over explicit instructions, with nothing special-cased in the code
  writer.
- **Rust MIR.** A place is a local plus projections (`Deref`,
  `Field(i)`, `Index(local)`). An assignment writes a place, and `&mut p`
  takes the place's address. Places are symbolic because MIR is generic.
  Offsets appear only in codegen, after monomorphization.

Two lessons apply to hd:

- A generic IR names places symbolically (Rust MIR). A concrete IR, after
  layout, computes them as addresses (Go, SIL).
- Counts belong in the IR as instructions (SIL), not in the code writer.

## The Proposal

### Two levels, each where it belongs

1. **TIR stays the checker's generic IR, with real places.** The checker
   already knows every place: it builds `RefPlace` records today. TIR
   gets three place forms that replace the value-rebuilding ones:
   - `Read(place)`;
   - `Write(place, value)`;
   - `AddrOf(place)` for `&` and `&mut`.

   A place is a root (a local, a global, a deref of a reference local, a
   list element) plus a field path, which is the `RefPlace` record made
   general. `CopyData` for field writes, the reference closure forms,
   `ListFieldSet` and `ListIndexMut` go. Calls that change a collection
   (`push`, an element write) take `&mut` to its place, as any `&mut
   self` method does.

2. **A new memory IR (MIR) per instance, after collection.** Types are
   concrete and layouts are known. MIR is what the emitter reads, and the
   emitter becomes a plain writer of Wasm instructions.

### MIR's shape

- **Structured control flow**, shaped like Wasm: `Block`, `Loop`, `If`,
  `Br`, `BrIf`, `BrTable`, `Return`. There is no CFG and no relooper.
  Binaryen shows that dataflow passes work on this shape.
- **Values and locals.** An instruction's result is a single-assignment
  value of one machine type (`i32`, `i64`, `f32`, `f64`); a pointer is an
  `i32`. A local is a mutable variable of one machine type. As in Go's
  `canSSA`, an hd local whose address is never taken becomes one MIR
  local per layout leaf. A taken one gets a memory slot.
- **Memory**:
  - `Slot(layout)`, which is a frame or shadow-stack slot, or a counted
    cell when it must outlive the frame;
  - `FieldAddr(p, offset)` and `IndexAddr(base, i, elem_size)`;
  - `BoundsCheck(i, len)`;
  - `Load(type, p)`, `Store(type, p, v)` and `Copy(dst, src, layout)`.
- **References** are pointers: `AddrOf(place)` lowers to address
  arithmetic, and a read is a load. The page-table interior-address
  model (§23.5) is unchanged.
- **Counts** are instructions:
  - `Retain(p)` and `Release(p, glue)`;
  - `Unique(slot_addr, clone_glue)`, the COW check that copies the buffer
    and stores the copy back into the place when the count is over 1;
  - `Pin` and `Unpin` as plain counts on a buffer.
- **Runtime primitives only.** What remains is `Alloc`, `Free`, host
  calls, and arithmetic with explicit panic checks. Every collection and
  string operation is hd code in `lib/std` over those primitives, as Go's
  maps and Rust's `Vec` are library code. Inlining small std functions
  gives back the fast paths that intrinsics give today.
- **One loop.** A `for` is `Loop` plus the `Iterable` cursor. The list
  fast path comes from inlining `ListIter.next`: a load of the length and
  an `IndexAddr`. No snapshot intrinsics are needed.

### Ownership as passes over MIR

Lowering inserts counts conservatively:

- a counted value copied out of a place is retained;
- a value stored over a counted one releases the old one;
- a value is released when its last owning value dies.

Then passes over the explicit instructions remove counts, each a small
dataflow rule:

1. **Borrow** (#676): a load whose value is used before any instruction
   that may store to its place or free its object needs no retain. That
   also drops the matching release. "May store" is explicit in MIR: a
   `Store` to an overlapping address, or a call that receives a pointer
   reaching the place or can reach it through a global or a cell.
2. **Pair cancellation:** a `Retain(x)` and a later `Release(x)` with no
   possible free of `x` between them both go.
3. **Move at last use:** a copy out of a place that is never read again
   becomes a move. The place is zeroed, or the zeroing is dropped when the
   exit no longer reads it.
4. **Known-null releases** (#677): a release of a value proven zero goes.
5. **Hoist `Unique`:** in a loop with no possible new owner, one check
   before the loop is enough. This is §23.31's buffer cache, now a real
   pass.

`count.rs`, `live.rs` and the loans of §23.31 become these passes. The
stage `counts` view reads MIR directly.

### What stays in the emitter

- Suspension (the state machine of §14) stays at first. It moves to a MIR
  pass once the rest works, because frames are slots and a resume is a
  `BrTable`.
- Erased generic bodies (§23.27) take the element size and glue from the
  runtime descriptor. MIR has dynamic-size forms of
  `IndexAddr`/`Copy`/`Load` for them.
- Wasm-level peepholes (`opt.rs`) stay as the final cleanup.

## How Each Known Defect Goes Away

| Case (known-bad stage fixtures, #656 catalog) | Today | With MIR |
| --- | --- | --- |
| `body.x = 1.0` on a local | `CopyData` + whole `LocalSet` | one MIR local write (the field's leaf local) |
| `body.x = v` through `&mut Body` | read via getter, `CopyData`, write via setter | `Store(f64, FieldAddr(body, 0), v)` |
| `xs[i].vx = v` | `ListFieldSet` special intrinsic | `Unique`, `BoundsCheck`, `Store(FieldAddr(IndexAddr(...)))`; `Unique` hoisted out of the loop |
| read of `&mut List` | getter closure call | `Load(i32, list_ref)` |
| `items.len()` | `ListLen` intrinsic | inlined std: `Load(i32, FieldAddr(buf, 4))` |
| `for x in list` | snapshot + index desugaring | cursor; inlined `next` is a length load plus `IndexAddr` |
| `for (k, v) in map` | `MapSlots`/`MapSlotNext` intrinsics | the map cursor in std |
| struct holding a list copied | `ValueCopy` intrinsic | `Copy` + `Retain` of the counted leaf, removed when the source dies right after |
| element method call | `ListPin`/`ListUnpin` + closure record | `AddrOf(elem)` + `Pin`; pair cancellation drops it when the call cannot change the list |
| fannkuch: 4 retains, 8 releases per swap | emitter counts every read | the borrow pass: none |
| binary_trees: releases of null locals | emitter releases every place at exit | known-null and move passes |
| i32 `%` widened to i64 | emitter arithmetic | fixed at the source (#682 already) |

## The List Representation

A list is a header over a buffer only because TIR changes lists through
their value. With places in TIR, `push` writes the new buffer address back
to its place, so a list can be the buffer's address, as Go's slice
pointer is. That saves one object and one load per access.

Pins need care. Today the header's owner word tells a pin from a COW
share. Without a header, a pinned buffer must still refuse to move. The
plan keeps the header through the migration and decides this after MIR
works. The decision needs a measurement, so it is not a guess made now.

## Migration

The new path runs beside the old one, per body:

1. **MIR crate and view.**
   - Add the data structure, a printer, and a `mir` stage view.
   - Lower a subset of TIR (scalars, structs, locals, places through
     references, list element access through today's intrinsics, calls)
     and emit Wasm from it.
   - A body with anything else uses the old emitter.
   - Gates: the whole conformance suite and the stage cases.
2. **Ownership passes on MIR**, which retire `count.rs`, `live.rs` and
   the loans. Bench gate: no program slower.
3. **Places in TIR:** `Read`, `Write` and `AddrOf` replace `CopyData`
   rebuilds, reference closure forms, `ListFieldSet` and `ListIndexMut`.
4. **Intrinsics move to std:** list, map and string operations become hd
   over the primitives, with inlining of small functions. The intrinsic
   tags and the snapshot and slot intrinsics go, and so does the dead
   `For*` tags.
5. **Every body on MIR.** Suspension moves to MIR, and the old emitter
   path is deleted.
6. **List as buffer address**, after measuring the pin design.

Each step lands only when its stage-case diffs show the improvement and
the bench shows no regression.

## Cost Model

- **Compile time:** one more IR per instance. MIR is flat and per
  instance, like the bytes it replaces, so the cost is one more walk.
  Codepack caching (§13.8) caches the output, not MIR.
- **Code size:** inlining std collection code can grow hot functions.
  The inliner gets a size budget, and hello's footprint guard applies.
- **Run time:** the borrow and pair passes remove the counts that cost
  most today (#656 ranks them first). Field writes become single stores.
  Loads of the list header disappear with step 6.
- **Risk:** two backends during the migration. The per-body fallback
  keeps every suite green at every step, and the stage cases show each
  body's new output.
