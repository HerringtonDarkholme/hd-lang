# TIR v2: Places And Explicit Ownership

Status: detailed design, 2026-10-21, for issue #667. Nothing here is in
the specification or the compiler yet; the owner approves before any
implementation. The owner chose Direction 3 of
[tir-v2-directions.md](tir-v2-directions.md), which stays as the record
of the alternatives. This document is the full design of that direction:
the IR, what the checker emits, the ownership passes, the emitter, a
staged migration with stage 1 ready to implement, and the cost.

Under review: TIR ([crates/hd_tir/src/ir.rs](../../crates/hd_tir/src/ir.rs),
[crates/hd_tir/DESIGN.md](../../crates/hd_tir/DESIGN.md) §4.13.11), the
checker's TIR construction (`hd_check/src/refs.rs`, `value.rs`,
`pins.rs`, `coll.rs`, the loop lowerings in `expr.rs`), collection
(`hd_mono`), and the linear backend
([crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
§23; `emit.rs`, `emit/count.rs`, `emit/live.rs`, `emit/refs.rs`,
`emit/list.rs`, `emit/closure.rs`, `emit/susp.rs`).

## Contents

1. [Summary](#summary)
2. [Starting points](#starting-points)
3. [The IR](#the-ir)
4. [What the checker emits](#what-the-checker-emits)
5. [The ownership passes](#the-ownership-passes)
6. [The emitter after each stage](#the-emitter-after-each-stage)
7. [Stage 1 in full](#stage-1-in-full)
8. [Stage 2](#stage-2)
9. [The inliner](#the-inliner)
10. [Stage 3](#stage-3)
11. [Cost](#cost)
12. [The known-bad cases](#the-known-bad-cases)
13. [Risks and rollback](#risks-and-rollback)
14. [Where I would revise an owner decision](#where-i-would-revise-an-owner-decision)

## Summary

TIR v2 is today's TIR with one new idea and one new duty.

The idea is the **place**: a root (a local, a top-level binding, an
instruction's value, or later a frame slot) followed by projections
(dereference, field, list element, enum variant), as in Rust's MIR. A
place is an operand, not a value. Three instructions use it: `Read`
gives the value at a place, `Store` replaces it, and `Ref` takes its
address. A field write through `&mut Body` becomes one instruction,
`Store [*$0 .x] 1.0`, where today it is 41. A field read through a
reference is one `Read`, not a closure call. Struct field writes no
longer rebuild their parents.

The duty is **visible ownership**. Each `Read` says how it reads: `copy`
(takes a count), `move` (takes the place's count and leaves it empty)
or `borrow` (no count). `Drop`, `IsUnique` and `Pin` are statements.
The checker writes none of this: it emits plain reads. A fixed sequence
of six passes in `hd_tir` (insertion, borrow, move, pair cancellation,
known-null, hoisting) decides every count once per generic body. They
replace the emitter's `count.rs` and `live.rs` decisions, its three loan
kinds and the checker's copy and pin bookkeeping, and they run before
any instance is emitted.

Everything else stays: structured control flow, the flat column
encoding, the wire format and its per-body hash, suspension lowering,
erased generic methods, layouts, the cycle collector and Wasm-level
peepholes. The migration has three stages, each with every suite green:
places (stage 1, about 10 one-hour commits), ownership passes (stage 2,
about 14), and collections in std over typed memory after an inliner
exists (stage 3).

## Starting points

**Fixed by the owner.** RC on wasm32 linear memory with the cycle
collector, never Wasm GC. A reference is one thin address. No
whole-program or cross-function analysis (#702 and #703 stay held). No
second copies of a callee for result demand (#681). Bounds-check
elimination is separate (#711). The bit-0 tag on interior references
(#704) is decided after stage 2; [Stage 2](#stage-2) says what places
make cheaper. TIR v2 replaces TIR: the checker emits it, the emitter
consumes it, and nothing sits between them. Compile time, memory,
compiler size and cognitive load count as much as run time.

**Decided by the owner (2026-10-21): the checker does not do reference
counting.** It emits places, plain reads, stores and references. All
count work (copies, drops, uniqueness checks, pins, then borrows, pair
cancellation, moves and known-null) is done by passes over the same TIR
v2 in `hd_tir`. There is no second IR, and the checker holds no count
rule. [Should the checker mark move, copy and borrow?](#should-the-checker-mark-move-copy-and-borrow)
settles the one open point in this decision: the checker marks no read
mode at all.

The working assumptions below are the orchestrator's recommendations.
The design follows them, with two refinements that the table states.

| Assumption | Why | If decided otherwise |
| --- | --- | --- |
| **Staged migration**, every suite green at each step: stage 1 places, stage 2 ownership passes, stage 3 collections in std after an inliner | Each stage deletes code and is measurable on its own. Stage 1 is mostly deletion; stage 2 is the large one; stage 3 depends on work that does not exist (the inliner, typed memory primitives) | One big switch would need the old and new paths side by side for 25 to 35 commits, with no green point to measure in between |
| **Stage 1 keeps plain local reads as they are** (refinement). `LocalGet`, `Field`, `TupleGet`, `Payload`, `Unwrap` and `ListIndex` stay until stage 2; stage 1 introduces `Read` only for reads through a dereference, where today's `RefRead` is | `live.rs` and `count.rs` match on `LocalGet` and `Field` chains in about 130 places. Changing every read in stage 1 rewrites the loans twice: once in stage 1, and again when stage 2 deletes them | Stage 1 grows by about 4 commits, and its baselines show read changes mixed with the reference changes |
| **The checker stays simple** | Decided by the owner, above | — |
| **The 24 intrinsics and today's list header stay through stages 1 and 2**, except the ones a place replaces (refinement). `ListSet` and `ListFieldSet` go in stage 1, since element writes become place stores; `ValueCopy`, `ListPin`, `ListUnpin`, `ListIndex` and `ListIndexMut` go in stage 2, since they become copy reads, pin statements, element reads and a uniqueness check before an element read. The other 17 stay to stage 3 | Stage 1's "element writes are place stores" and "the intrinsics stay" cannot both hold for the store intrinsics. Keeping an intrinsic that a place replaces would keep two forms of one write | Keeping all 24 through stage 2 means `xs[i].x = v` keeps its special intrinsic and the emitter keeps `list_field_set` beside the general place store |
| **Generic bodies keep symbolic places**: a projection is a field index, and the emitter folds a chain to base plus constant offset in its per-instance walk. For a type parameter, count operations stay symbolic, and the emitter drops them for scalar instances | `emit/refs.rs` already folds a field path this way (`ref_place`, `path_step`). No lowering pass is needed | A per-instance lowering pass would add a stage between check and emission and store a second form per instance |
| **Tier-2 hooks** (docs/future-work/OBJECT_MODEL_TIER2.md, #426, not adopted): a frame-slot root, lifetime regions on borrows, borrow-flag statements, all unused in tier 1 | The #667 claim is that tier 2 needs no further IR redesign; the hooks are the parts of the encoding it would need | Without them, tier 2 adds a root kind, a column and two tags later. That is a small extension, not a redesign, so the claim would still hold |

## The IR

### What stays

Most of TIR is unchanged and keeps its documentation in
crates/hd_tir/DESIGN.md §4.13.11:

- **The columns.** A body is the `Body` struct of flat columns: `tags`,
  `data` (two words per instruction), `ty`, `syn`, `extra` (records:
  a length word, then words), the local, sub-body, label, capture,
  suspension, constant and string columns.
- **Values are instructions.** An instruction's result is a value that
  later instructions name with a `Ref`; constants are `Ref`s with bit 31
  set. A value is a temporary: it is read-only and lives until its last
  use.
- **Structured control.** `Block`, `If`, `Loop`, `Break`, `Continue`,
  `Return`, `Scope`, `Defer`, `Match` with its decision tree (`SwitchTag`,
  `SwitchInt`, `SwitchChar`, `SwitchStr`, `Guard`, `ToArm`), `And` and
  `Or`. Wasm wants structured control, and suspension lowering numbers
  its states in tree order (`susp::plan`).
- **Calls, closures, suspension, providers and coercions.** `Call`,
  `CallValue`, `CallDyn`, `CallHost`, `Intrinsic`, `DefaultCall`,
  `Interp`, `Closure` with its capture columns, `Await`, `AwaitValue`,
  `AwaitAll`, `AwaitRace`, `Hook`, `With`, `ProviderGet`, `ContextNew`,
  `ContextFor`, and `Coerce` with its thirteen kinds.
- **Constructions and the rest.** `NewData`, `NewVariant`, `NewTuple`,
  `NewList`, `NewMap`, `ItemRef`, `Is`, `Hole` and `Poison`.

### Places

A place names where a value lives. It is a **root** and zero or more
**projections**, read left to right.

| Root | Written | Meaning |
| --- | --- | --- |
| local | `$3` | a parameter, binding or hidden local of the body |
| global | `@app.main.counter` | a top-level binding, with its type |
| value | `%7` | an instruction's value: a temporary, read-only unless a dereference follows |
| frame slot | `^2` | tier 2 only: a slot of the function's frame (reserved) |

| Projection | Written | Meaning |
| --- | --- | --- |
| dereference | `*` before the root or after a projection | the place a reference value refers to; also the payload of a compiler-supplied data object, which is a counted address |
| field | `.x` | a struct field or tuple element, by declaration index; embedded fields are explicit steps |
| element | `[%5]` | a slot of a list's buffer at the index value `%5`, bounds-checked; in stages 1 and 2 the projected value is the list header |
| variant | `as .Circle` | stage 2: the payload of an enum value known to be that variant, before its fields |

Examples, with the source they come from:

| Source | Place |
| --- | --- |
| `body.x` where `body: &mut Body` | `[*$0 .x]` |
| `nested.body.x` on a local | `[$1 .body .x]` |
| `bodies[i].vx` where `bodies: &mut List[Body]` | `[*$0 [%5] .vx]` |
| `f().x` where `f` returns `&Point` | `[*%3 .x]` |
| a field of a data object held in `$4` | `[*$4 .name]` |
| the payload `r` of `.Circle(r)` matched in `$0` | `[$0 as .Circle .r]` |

Two places **overlap** when one's projection list is a prefix of the
other's after their roots match, ignoring element indices (two elements
of one list may be the same slot). Overlap is the one aliasing test the
passes use; a dereference of a reference that came from elsewhere
overlaps every place of its referent type that a reference can reach.

A place's intermediate types are not stored. The instruction's `ty` is
the place's type; each step's type is the base type's declared field
type, substituted, which the emitter already computes per instance while
it folds offsets (`path_step`).

### Reads, stores and references

| Tag | Operands | Type | Meaning |
| --- | --- | --- | --- |
| `Read` | a: place, b: mode | the place's type | the value at the place. Mode `use` is the checker's; the passes replace it with `copy`, `move` or `borrow` |
| `Store` | a: place, b: value | `void` | replaces the value at the place: the new value moves in, the old one is dropped. Known-null marks a store into an empty place as an initialization, which drops nothing |
| `Ref` | a: place, b: `0` shared or `1` mutable | `&T` or `&mut T` | the address of the place. A local root makes the local addressable (a cell today; a frame slot in tier 2) |

**Read modes.** A value produced by `copy` or `move` owns one count of
each counted leaf it holds; a `borrow` value owns none and is valid
while its place is unchanged.

| Mode | Count | The place afterwards | Who writes it |
| --- | --- | --- | --- |
| `use` | undecided | unchanged | the checker |
| `copy` | one more of each counted leaf (a list header copy in stages 1 and 2) | unchanged | insertion |
| `move` | the place's own | empty: zeroed, or known dead | move |
| `borrow` | none | must not change while the value lives | borrow |

For a type with no counted leaf every mode costs nothing, and the
emitter emits the same loads for all four.

**Where values are owned.** A call's result, a construction, a `copy` or
`move` read, a `Ref` of a celled local, `Interp` and a `Closure` produce
owned values. A use of a value either **consumes** it (a `Store`, an
owned argument, a construction part, a `Return` or `Break` value, a
capture) or **lends** it (a borrowed argument, an operand of `Prim`, an
element index, the root of a place). After the passes, every owned
value has exactly one consumer or one `Drop`.

### Ownership statements

These arrive in stage 2. The checker never writes them.

| Tag | Operands | Meaning |
| --- | --- | --- |
| `Drop` | a: place | releases the value at the place and leaves it empty. A drop of an empty place does nothing at run time, so a drop needs no flag of its own: a moved place is zero |
| `IsUnique` | a: place of a list or map | makes the collection's buffer the place's own: copies it when another holder shares it. Precedes every in-place change |
| `Pin` | a: place of a list | after an `IsUnique` of the same place, marks the header as its buffer's one owner, so element references keep the buffer in place. The pin lasts while element references hold counts of the buffer; their drops end it, so there is no `Unpin` |
| `FlagSet`, `FlagClear` | a: place | tier 2 only (reserved): set and clear a borrow flag next to a pin |

**The exit set.** A body has one exit (`Return` branches to it, as
today). Its `exit_drops` column lists the owned locals that may still
hold a value there; the exit drops them. Insertion fills it, and
known-null removes the locals that are empty on every path to the exit.
A `Break` or `Continue` that leaves a block gets explicit `Drop`s of
what that block still owns, before the jump.

### The catalog

Tags by stage. "Gone" means no producer and no tag.

| Group | Stage 1 | Stage 2 | Stage 3 |
| --- | --- | --- | --- |
| places | `Read` (through a dereference), `Store`, `Ref` added | `Read` for every read | same |
| plain reads | `LocalGet`, `GlobalGet`, `Field`, `TupleGet`, `Payload`, `Unwrap` stay | gone: `Read` with a local, global, field or variant projection | same |
| plain writes | `LocalSet`, `GlobalSet`, `FieldSet` gone: `Store` | same | same |
| references | `RefPlace`, `RefRead`, `RefWrite` gone; `RefDyn` and `RefIdentity` lose their closure-form blocks | same | same |
| data | `CopyData` stays only for copy-update literals | `CopyData` gone: `NewData` of field reads | same |
| ownership | none | `Drop`, `IsUnique`, `Pin`; `FlagSet` and `FlagClear` reserved | same |
| loops | `ForRange`, `ForList`, `ForMap` gone (no producer today) | same | one `for` lowering through `Iterable` |
| intrinsics | 22: `ListSet` and `ListFieldSet` gone | 17: `ValueCopy`, `ListPin`, `ListUnpin`, `ListIndex`, `ListIndexMut` gone | memory primitives only |

After stage 2 the catalog has 56 tags, two of them reserved; today's has
64. Of the five reference tags with closure-form blocks, the hardest
part of today's catalog, three go and two keep only their value form.

### Encoding, wire format and caching

**The place word.** A place operand is one data word. With bit 31 set,
it is a bare local `$n` (bits 0 to 30), with no record: today's
`LocalGet` costs nothing more as `Read [$n]`. With bit 31 clear, it is
the offset of a place record in `extra`:

```text
[len, root kind, root word, (root word 2), projection word ...]
  root kind 0 local     root word: LocalId
  root kind 1 global    root words: DefId, Ty         (ID words, remapped by the wire)
  root kind 2 value     root word: Ref
  root kind 3 frame     root word: slot               (tier 2, reserved)
projection word: top 2 bits the kind
  00 field     bits 0..29: field index
  01 deref     no payload
  10 element   the next word is the index Ref
  11 variant   bits 0..29: variant declaration index   (stage 2)
```

`Read` keeps its mode in data word b. Lifetime regions (tier 2) live in
a side column `regions: Vec<(u32, u32)>` of (instruction, region),
sorted, empty in tier 1, so tier 1 pays nothing. `exit_drops` is a pass
output and never reaches the wire.

`body.x = 1.0` through `&mut Body` is then one instruction (17 bytes)
and a five-word record (20 bytes), where today it is 41 instructions,
two hidden locals and two closure sub-bodies. `Point.shift`'s
`self.x += dx` is laid out as:

```text
local $0 self: &mut Point [param]   local $1 dx: i32 [param]
extra:  @0 [4, 0, 0, 1<<30, 0]       # $0, deref, field 0 (x)
%0 = Read      a=@0  b=use   : i32
%1 = LocalGet  a=$1          : i32  # stage 2: Read a=$1|bit31
%2 = Prim Add  [%0 %1]       : i32
%3 = Store     a=@0  b=%2    : ()
```

The read and the store share one record: a place record is immutable
data, and a place's root and indices are values evaluated once.

**Wire format.** `write_body` and `read_body` keep their layout: per-body
tables, then the columns, with ID words of `extra` remapped by
`id_words`. The changes are local:

- `id_words` learns the place record: a global root's `DefId` and `Ty`
  are ID words, every other word is copied. `map_body_types` and
  `map_ids` follow the same list. Since instructions share place
  records, a body's ID words are collected once per `extra` position
  before they are remapped; mapping a shared word twice would remap a
  table row as if it were an ID.
- `RefPlace`'s special case for a global root goes with `RefPlace`.
- The tag numbers change, so every body's TIR hash changes once. The
  toolchain key changes with the compiler anyway, so no stale entry is
  ever read.
- `strip_closure_forms` and its hash salt in the driver's `decode`
  (`"ref-addresses"`) go: the bodies arrive without closure forms.

Caching is unchanged: the `check` entry stores the checker's TIR (modes
`use`, no ownership statements), and collection decodes the bodies it
reaches. The passes run after decoding ([where and when the passes run](#where-and-when-the-passes-run)),
so the wire never carries pass output and a pass change needs no cache
format change.

### Invariants

The verifier keeps invariants 1 to 17 and adds these four, numbered 20
to 23 in `crates/hd_tir/DESIGN.md` §4.13.11's list:

1. **Places are well formed.** A local root names a local of the body;
   a value root precedes its user and is visible there; an element
   index precedes its user; a frame root occurs only in tier 2.
2. **Stores and mutable references reach writable places.** A `Store`
   or a `Ref` with `b = 1` has a local or global root, or a dereference
   in its projections. A value root is never written in place.
3. **Checker output has no ownership.** Before the passes every `Read`
   has mode `use` and there is no `Drop`, `IsUnique`, `Pin` or
   `exit_drops` row.
4. **After the passes, ownership balances** (checked in debug and
   checked builds): every owned value has exactly one consumer or one
   `Drop` on each path; no `Read` of a place follows its `move` on any
   path without a `Store` in between; no `borrow` value is live across
   a write to an overlapping place or a suspension point.

### Tier-2 hooks

[OBJECT_MODEL_TIER2.md](../future-work/OBJECT_MODEL_TIER2.md) is not
adopted. The IR reserves what it would need, so the #667 claim can be
checked when tier 2 is designed:

| Tier 2 needs | TIR v2 provides |
| --- | --- |
| scoped references `&{a, self.x} T` with no counts | `borrow` reads and `Ref` of a place, with a region in the `regions` column |
| NonCopyable values, moved and released at scope end | `move` reads and `Drop`; a read of a NonCopyable place is a `move` by its type (see [the checker's marks](#should-the-checker-mark-move-copy-and-borrow)) |
| static exclusivity from locals | the overlap test on places, the same one the borrow pass uses |
| borrow flags for borrows through global references | `FlagSet` and `FlagClear` next to `Pin` |
| non-escaping locals and closure environments in the frame (#702) | the frame-slot root |
| facts from signatures, never bodies | the passes already read only callee signatures |
| one compile per declaration, lifetimes erased | the passes run per generic body; places resolve per instance |

The reserved parts cost a root kind number, two tag rows that the
verifier rejects, and an empty column.

## What the checker emits

The tables show today's TIR, taken from the stage fixtures (the
`known-bad-tir` and `checker` cases under `crates/hd_check/tests/cases`)
and from scratch cases dumped for this design, then stage 1's and stage
2's checker output. Ownership appears only after the passes, shown where
it matters. Notation: `$n` a local, `%n` a value, `[...]` a place;
locals introduced only for closure forms are left out of "after".

### Locals and `let mut`

A `let` binding, `:=` and a `let mut` are all locals; `mut` matters only
to the checker's writability rules. Today `let i = n; i = i - 1`:

```text
%0 = LocalGet $0 : i32
%1 = LocalSet $1 %0 : ()
%4 = LocalGet $1 : i32
%5 = Prim Sub [%4 1:i32] : i32
%6 = LocalSet $1 %5 : ()
```

Stage 1 writes `Store [$1] %0` and `Store [$1] %5`. Stage 2 writes
`Read [$0]` and `Read [$1]` for the two reads. A local declared with no
initializer is simply not stored until its first `Store`; known-null
turns that first store into an initialization.

### Field reads

A field of a struct local stays `LocalGet` plus `Field` in stage 1 and
becomes one read in stage 2: `p.x` is `Read [$0 .x]`. Through a
reference it changes in stage 1. Today `fn by_ref(p: &Point) -> i32: p.x`:

```text
%0 = LocalGet $0 : Ref[Point]
%1 = Field %0 .0 : fn() -> Point        # the getter closure
%2 = CallValue %1 [] : Point
%3 = Block %2 : Point
%4 = RefRead %0 %3 : Point
%5 = Field %4 .x : i32
```

After, in both stages:

```text
%0 = Read [*$0 .x] : i32
```

The emitter loads only `x` (today's "selected field" rule of §23.31
becomes the meaning of the place, not an optimization).

### Field writes

Local root, today (`a-nested-field-write-rebuilds-each-enclosing-value`):

```text
%2 = LocalGet $1 : Nested
%3 = Field %2 .body : Body
%4 = LocalGet $1 : Nested
%5 = Field %4 .body : Body
%6 = CopyData %5 [.x = 1.0:f64] : Body
%7 = CopyData %4 [.body = %6] : Nested
%8 = LocalSet $1 %7 : ()
```

After: `Store [$1 .body .x] 1.0:f64`.

Through `&mut self`, today (`Point.shift`, `self.x += dx`):
44 instructions, two closure sub-bodies, a `Ref[mut i32]` object, a
`StrConcat` for its identity and two hidden locals. After, as laid out above:
`Read [*$0 .x]`, `LocalGet $1`, `Prim Add`, `Store [*$0 .x] %2`.

A top-level binding: `Store [@app.main.total .count] v`. A field of a
compiler-supplied data object, today a `FieldSet` on the object value:
`Store [*$4 .name] v`.

### Element reads and writes

Element reads stay `Intrinsic ListIndex` in stage 1, because the
emitter's loans match on it, and become `Read [$0 [%1]]` in stage 2.
Element writes change in stage 1. Today
`bodies[i].x = bodies[i].x + dt * bodies[i].vx`
(`a-struct-field-write-through-a-list-element`) reads `bodies` through
its getter three times and ends in `Intrinsic ListFieldSet [%4 %5 %25]
.x`. After stage 1:

```text
%0 = LocalGet $1 : usize
%1 = Read [*$0] : List[Body]
%2 = Intrinsic ListIndex [%1 %0] : Body
%3 = Field %2 .x : f64
%4 = LocalGet $2 : f64
%5 = Intrinsic ListIndex [%1 %0] : Body
%6 = Field %5 .vx : f64
%7 = Prim Mul [%4 %6] : f64
%8 = Prim Add [%3 %7] : f64
%9 = Store [*$0 [%0] .x] %8 : ()
```

After stage 2 the two reads are `Read [*$0 [%0] .x]` and
`Read [*$0 [%0] .vx]`, each loading one field. Here `*$0 [%0]` means:
the list `$0` refers to, its buffer, slot `%0`.
A store through an element projection makes the buffer unique first.
In stage 1 the emitter does that, as `list_field_set` does today; in
stage 2 it is an explicit `IsUnique [*$0]` that the hoisting pass can
move out of a loop. A whole-element store `xs[i] = v` is
`Store [$0 [%1]] v`, replacing `ListSet`.

Map values are no places (`m[k].x = v` is an error today and stays one).
`m[k]` reads stay `MapIndex`, `m[k] = v` stays `MapSet`, and
`m[k].push(v)` keeps `MapIndexMut`, until stage 3 moves maps to std.

### `&` and `&mut`

Today `r := &mut x; *r = 5` on a `let mut x` draws a reference serial
at `x`'s declaration (`Call std/core/ref_serial`, which the backend
folds to 0), builds `get` and `set` closures, a `RefRoot` identity, a
`Ref[mut i32]` object, then `RefPlace`, and writes through the `set`
closure: 26 instructions and two hidden locals. After:

```text
%0 = Store [$0] 1:i32 : ()
%1 = Ref mut [$0] : &mut i32
%2 = Store [$3] %1 : ()
%3 = LocalGet $3 : &mut i32
%4 = Store [*%3] 5:i32 : ()
%5 = LocalGet $0 : i32
```

`&e` on a value that is no place makes a new object: the checker binds
the value to a hidden local and takes `Ref [$k]` of it, as today but
without closures. An element reference `&xs[i]` is
`Ref [$2 [%5]]` on the pinned list local `$2`, with the index value `%5`
(an element index is always a value, evaluated once); in stage 2 the
checker emits only `Ref [*$0 [%5]]` and insertion adds `Pin [*$0]`
before it
([insertion](#insertion)). `*r` as a whole value is `Read [*$0]`;
`*r = v` is `Store [*$0] v`.

Identity (`is`, `identity_of`) needs nothing in the IR: the backend
compares addresses (§23.20), so the serials, `RefRoot` and identity keys
go.

### Calls and pass modes

The pass mode of an argument is its parameter's type, which the passes
read from the callee's signature. The checker emits the argument values
in order, as today.

| Parameter | The checker passes | After the passes |
| --- | --- | --- |
| `p: T` (an owned copy) | `Read [place]` or any value | a `copy` read, or `move` at the place's last use; a temporary moves in |
| `mut p: T` (the callee's changeable copy) | the same | the same; the callee owns and may write its local |
| `p: &T` | `Ref [place]`, or a reference value | the reference is lent: neither side counts it |
| `p: &mut T` | `Ref mut [place]` | lent; the place must be writable (the checker's rule) |
| `&self`, `&mut self` method on a place | `Ref [place]` (auto-address) | lent |
| `&self` method on a temporary | a hidden local and `Ref [$k]` (a new object) | lent; the hidden local is dropped after the call |

Today's `calls` function from the scratch case, `a.shift(1)` on a
`let mut a`, builds a whole reference object (instructions `%4` to
`%16`); after, it is `%2 = Ref mut [$1]` and the `Call`. `q.shift(2)` on
`q: &mut Point` passes `LocalGet $0` unchanged in both forms.
`by_ref(&a)` becomes `Ref [$1]`. `by_value(a)` and `by_mut(a)` pass
`LocalGet $1` (stage 2: `Read [$1]`), which the passes make a copy or a
move.

A bound method reference `counter::bump` stays a `Closure` over a
hidden local holding the receiver.

### Closures and captures

Unchanged. A closure is a sub-body in the same columns; its captures
and their modes (`Copy`, `Move`, `Shared`) are the capture columns, and
reads inside the sub-body name the outer local. A `Shared` capture
lives in a cell, as an addressed local does. Today `counter`:

```text
%0 = LocalSet $0 0:i32 : ()
%1 = LocalGet $0 : i32           # inside sub-body 1
%2 = Prim Add [%1 1:i32] : i32
%3 = LocalSet $0 %2 : ()
%4 = LocalGet $0 : i32
%6 = Closure #1 [%0 %1] : fn() -> i32
```

After: the `LocalSet`s become `Store [$0]`, and in stage 2 the reads
become `Read [$0]`. The passes run per sub-body, with the captures as
locals that hold a value on entry.

### Match and patterns

The decision tree is unchanged. Payload reads become places in stage 2.
Today `.Circle(r) => r * r` on `s: Shape`:

```text
%8 = Payload %0 [#1 #0] : f64
%9 = LocalSet $1 %8 : ()
```

After stage 1: `Store [$1] %8`. After stage 2:
`%8 = Read [$0 as .Circle .r]` and `Store [$1] %8`. `Unwrap` of an
optional is `Read [$0 as .Some .0]`. A match on an owned temporary
(`match parse(text):`) roots its payload reads at the temporary,
`[%3 as .Ok .0]`, which is what lets the move pass move one payload out
and drop only the rest (#679).

### `for` and `while`

`while` is unchanged in shape: a `Loop` whose body is
`If c then body else Break`. `for` keeps today's three lowerings
through stages 1 and 2: a range as a counting loop, a list or map as a
counted loop over a snapshot (`SnapBegin`, `ListLen`, `ListIndex`,
`SnapEnd`; `MapSlots` for a map), and anything else through `iter` and
`next`. The only stage-1 change is that the cursor local's
`&mut` reference loses its closure form. Stage 2 rewrites the element
read as `Read [$3 [%p]]`, where `%p` reads the position local. Stage 3 replaces all three with one lowering
through `Iterable` ([Stage 3](#stage-3)).

### `defer` and `using`

Unchanged: a cleanup `Scope` lists its suites, and a `Defer` registers
one. `using x = e` is a `Scope` whose suite calls `dispose` (or
`dispose!`) on the binding. The checker's `using.rs` takes a reference
to the binding through `make_ref` and reads it through `emit_deref`, so
it gets `Ref` and `Read` like every other site.

The passes treat each suite as a use, at the scope's exits, of every
place it reads. Today the emitter gives up its loans and payload moves
in a body with a `defer` (`live::defers`), because a suite runs later
than it stands. With the suite's uses at the exits, that rule goes.

### Suspension

Unchanged. A bang call is `Await`, `s!()` is `AwaitValue`, `all!` is
cold `Call`s then `AwaitAll`, `race!` a `NewList` of cold suspensions
then `AwaitRace`. Today `twice!`:

```text
%0 = LocalGet $1 : i32
%1 = Await app/main/wait[] (%0) : i32
%2 = LocalSet $2 %1 : ()
```

After stage 1: `Store [$2] %1`. The passes treat a suspension point as
a barrier: no `borrow` value is live across one, since another task may
change the place while this one waits, and every value live across it
is in the frame (`susp.rs` saves every Wasm local).

### Providers

Unchanged: `With`, `ProviderGet`, `ContextNew`, `ContextFor`. A
provider of a key whose trait has a `&self` method is a trait value
reference; `$.use(Mailer)` is `ProviderGet` then `Coerce ProviderAsRef`.
A method call through it reads the trait value through the reference:
today `RefRead` with its getter, after `Read [*$1]`. A `With` block
lends its providers.

### String interpolation

Unchanged: `"hi $name #$n"` is `Interp ["hi " %0 " #" %2]`, where `%2`
is the `Display.to_string` call of `n`. In stage 2 `%0` is
`Read [$0]`, which the borrow pass makes a borrow, since `Interp` lends
its parts.

## The ownership passes

Stage 2 adds `hd_tir::own`, six passes over one body, in this order:
insertion, borrow, move, pair cancellation, known-null, hoisting. Each
pass reads the body and rewrites read modes or adds and removes
ownership statements; none changes what the program computes.

### Where and when the passes run

The passes run on each body collection decodes, once per body per run,
in the driver's `decode`, where `strip_closure_forms` runs today. All
instances of a generic body share the result.

- **Why after decoding, not in `ModuleFinish`.** At decode the program's
  layout facts are at hand (whether a type has counted leaves), and the
  passes run only on the bodies a program reaches: 231 of json's 2,335
  checked bodies. In `ModuleFinish` they would run on every body of
  every checked module, about ten times the work on a cold build, to
  save a cost that is small on a warm one (see [Cost](#cost)).
- **Why once per body, not per instance.** The passes read no instance
  types. A type parameter's values count as "maybe counted"; the
  emitter drops their count glue for scalar instances. Today
  `live::plan` reads no types either, yet it runs once per instance.
- **Fixtures.** The `tir` view keeps showing checker output. A new
  `own` view (in `hd_driver`, which can decode) shows a body after the
  passes, so every pass's decisions are reviewable as baseline diffs,
  the way `counts` shows the emitter's today.

### What the passes read

The passes read the body and a small interface the driver implements
from the program's tables:

```rust
pub trait OwnCx {
    /// Whether a value of `ty` holds a counted leaf: `No`, `Yes`, or
    /// `Maybe` for a type that mentions a type parameter.
    fn counted(&self, ty: Ty) -> Tri;
    /// How argument `k` of a call of `callee` is passed: owned (a
    /// value parameter) or lent (a reference parameter).
    /// `layout::Pass` today.
    fn pass(&self, callee: &Callee, k: usize) -> Pass;
    /// Whether a call of `callee` may reach memory outside its
    /// arguments: false only for the few std functions known pure
    /// (`sqrt` today).
    fn may_write(&self, callee: &Callee) -> bool;
}
```

These are signature and layout facts, never another body's, so the
passes stay per function.

### Should the checker mark move, copy and borrow?

**No.** The checker emits every read as `use`, and the passes choose
the mode. It still emits the two source forms that are not reads: `&p`
and `&mut p` (a `Ref`), and `*r = v` and other writes (a `Store`).

- **Every read in hd is a copy by its meaning.** All types copy (Go and
  Swift semantics); a move or a borrow is only an optimization whose
  legality depends on liveness and aliasing, which the checker does not
  compute and, by the owner's decision, should not. A mark the checker
  wrote would have no source to come from but the same analysis.
- **Cost.** None to add. Insertion visits every read anyway to place
  drops; giving each its mode there is one more word write. The checker
  loses code: `coll_copy`, `return_copy`, `loop_binding_copy`,
  `is_fresh`'s ownership uses and `reads_fixed_local` (about 200 lines
  of `coll.rs`), and every contributor stops having to know where a
  list header must be copied.
- **Soundness.** `copy` is always sound: it only adds a count. Each later
  pass weakens a mode only under its own proof, so a missed case leaves
  a count, never a dangling value. A wrong mark from the checker (a
  `move` of a place read again later) would be a use after free that no
  pass could see.
- **Precedent.** Rust's MIR builder marks `Copy` or `Move` by type (the
  `Copy` trait), not by liveness; in hd every type is copyable, so that
  rule says "copy" everywhere and needs no mark. Swift's SILGen emits
  copies conservatively and ownership optimization removes them; Lean 4
  and Koka (Perceus) insert their `inc`/`dec` in a pass over an IR with
  none.

The one future exception is tier 2's NonCopyable values: a read of
such a place is a `move` by its type, and a later read of it is a
source error. Insertion would then mark those reads `move` by type
(Rust's rule), and the use-after-move check would report through the
checker. That is a type rule, not a liveness one, and nothing in tier 1
needs it.

### Insertion

Insertion is the naive, correct ownership of §23.16 plus last-use
release, made explicit.

1. **Modes.** Every `use` read becomes `copy`. For a type with no
   counted leaf, `copy` is free; the later passes weaken the rest.
2. **Temporaries.** An owned value used by more than one consumer gets
   a `copy` for each extra consumer (`Read copy [%v]`), so each owned
   value has one consumer. An owned value with no consumer gets a
   `Drop [%v]` after the statement of its own block that holds its
   last use, as `live::plan` places a value's release today.
3. **Locals.** An owned local gets a `Drop [$l]` after the statement of
   the innermost block holding all its accesses that holds the last one
   (today's `live::plan` rule). Parameters and captures live in the
   sub-body's root block. A `Break` or `Continue` that leaves that block
   before the drop gets a `Drop` before it; the function's exit drops
   the rest through `exit_drops`.
4. **Defer suites.** A place a suite reads counts as used at every exit
   of the suite's `Scope`.
5. **Uniqueness.** Every in-place change of a collection place gets an
   `IsUnique` of that place just before it: a `Store` through an element
   projection, `ListPush`, a read of an element that a change then goes
   through (`xs[i].push(v)`, today's `ListIndexMut`), `MapSet`,
   `MapRemove`, `MapIndexMut`. The intrinsics stop checking for
   themselves.
6. **Pins.** A `Ref` of an element place gets `IsUnique` and `Pin` of
   its list place before it. The reference holds a count of the buffer;
   its drop (rule 3) ends the pin at its last use, which is the rule
   `pins.rs` implements today at statement granularity with flags and
   a `defer` per pin.

Why it is sound: every owned value and local is dropped exactly once on
every path. On the normal path, by the drop after the last use; on a
jump out of the block, by the drop before the jump; at the exit, by the
exit set, where a place already moved or dropped is zero and drops as
nothing. That is the invariant today's emitter relies on (§23.16 "One
exit", §23.20 "Releases at last use"), now visible.

### Borrow

Borrow turns `copy` reads into `borrow` reads where the value can live
on the place's count. A read borrows when all of these hold over its
**span**, the statements from the read to its value's last use:

1. **Every use lends.** No consumer needs a count of its own (an owned
   argument, a store, a return). A borrowed value has no count to give.
2. **Nothing writes an overlapping place.** No `Store`, `Drop`, `move`
   read, `IsUnique` or mutable `Ref` of a place that overlaps the read's
   place stands in the span.
3. **Nothing reaches the place behind the body's back.** For a local
   root: the local is never addressed (`Ref`), never captured, and lives
   in no cell, so only this body's own instructions change it. For a
   dereference of a `&T` or `&mut T` parameter: the caller keeps the
   referent alive for the call, and no call in the span may write
   (`OwnCx::may_write`). Any other dereference root borrows only over a
   span with no call at all.
4. **No suspension point** stands in the span.

Why it is sound: the place holds its own count across the whole span,
since rules 2 to 4 rule out every instruction that could drop or replace
its value, and rule 1 ensures nothing keeps the value past the span. A
change made in place through a list header (a push, an element store)
keeps the header, which is the value read in stages 1 and 2; that is
§23.36's existing argument, now one rule.

### Move

Move turns a `copy` read at its place's last use into a `move` and
deletes the place's `Drop`:

1. **Locals.** A `copy` read of `[$l]` is `$l`'s last access, its
   `Drop` follows that statement, and its value is consumed. The read
   becomes `move`, the drop goes, and `$l` is empty afterwards.
2. **Temporaries.** A value whose one consumer consumes it already moves
   (insertion gave it no drop); a `copy` of a temporary at that
   temporary's last use becomes the temporary itself.
3. **Partial moves.** A `copy` read of a projection of an owned
   temporary or local, such as `[%3 as .Ok .0]` in the arm that matched,
   at the base's last use, becomes `move`, and the base's `Drop` becomes
   drops of the base's other counted fields, or nothing (#679).
4. **References at last access.** A `Ref` of a celled local `$l` at its
   last access takes `$l`'s count of the cell, and `$l`'s drop goes
   (#701).

Why it is sound: the place is read nowhere after the move, so nothing
can see it empty; a jump past the move still drops the place, which
holds its value there, because the drop before a jump and the exit set
stay.

### Pair cancellation

Pair cancellation deletes a `copy` and a `Drop` of the same value when
nothing between them can tell: within one straight-line run of
statements, a `copy` of place `p` (or a `Ref` that counts a cell) and a
later `Drop` of a place holding that same value, with no write to an
overlapping place, no `IsUnique` or `Pin` that reads the count, no call
that may write, and no control edge in between. The copy becomes a
`borrow` and the drop goes.

It catches what move cannot: a value copied into a temporary that is
dropped at the end of the statement, while the place outlives it.
Wasm-level pair cancellation in `opt.rs` (§23.25, "Balanced pairs")
stays for the code the backend generates itself (glue, thunks), which no
TIR pass sees.

A `Ref` of an element place is never weakened by this pass or by move:
its count of the buffer is the pin.

Why it is sound: a count added and removed with no observer in between
changes nothing; the excluded statements are exactly the ones that
observe a count (uniqueness and pins read it) or could free the object
in between.

### Known-null

A forward pass over the structured control flow tracks, per local and
per temporary, whether it is certainly empty, certainly full, or
unknown. A place is empty before its first store, after a `move` and
after a `Drop`; full after a `Store`. It joins at the ends of `If` and
`Match` arms and at a loop head.

1. A `Drop` of a certainly empty place goes (#677).
2. A `Store` into a certainly empty place becomes an initialization:
   no old value to drop.
3. A local empty on every path to the exit leaves `exit_drops`.
4. A `move` whose source is certainly never read or dropped again needs
   no zeroing; the emitter skips the zero store.
5. A celled local whose declaration certainly ran (it dominates the use)
   is full, so its uses read the cell with no null test (#678).

Why it is sound: each rule removes work only on a place whose state is
certain on every path; an unknown state keeps the drop, the zeroing or
the test.

### Hoisting

Hoisting moves an `IsUnique` out of a loop (#653, #710). An `IsUnique`
of place `p` inside a loop moves to just before the loop, guarded by the
loop's entry test so an empty loop copies nothing, when:

1. the `IsUnique` runs on every iteration that is entered (no `If`,
   `Match` or inner loop around it skips it);
2. the loop never copies `p` or an overlapping place, takes no `Ref` of
   it, pins nothing, and holds no suspension point;
3. no call in the loop may write, unless `p`'s root is a local that is
   never addressed or captured.

The same proof marks the loop's **stable list places**: a list place
whose header nothing in the loop replaces. The pass records them in a
side table, and the emitter keeps their buffer address and length in
Wasm locals for the loop, as #710's list caches do today; with the list
header gone in stage 3, the buffer address is an ordinary read the pass
can hoist itself.

Why it is sound: a buffer made unique before the loop stays unique,
since nothing in the loop can make a second owner (rule 2) and nothing
outside can reach the place while the loop runs (rule 3).

### Why this order

Insertion makes every count explicit. Borrow runs next, because each
copy it removes is one that move and pair cancellation need not look at.
Move runs before pair cancellation: it handles the common case (a local
read at its last use) with a simple rule, and leaves pair cancellation
the rarer temporaries. Known-null needs the final moves and drops, so it
runs after both. Hoisting runs last, on the `IsUnique`s that survive.
Every pass after insertion only weakens or deletes, so stopping after
any prefix of the order still gives a correct program; that is how
stage 2 lands one pass per commit.

### What each emitter rule becomes

| Today (where) | In TIR v2 | Every behaviour kept |
| --- | --- | --- |
| §23.16 naive counting: every copy into a place counts, every place released at the one exit (`count.rs`) | insertion: `copy` reads, `Drop`s, `exit_drops` | the one exit and zeroing stay; counts are the same before the later passes |
| §23.20 releases at last use, per statement (`live::plan`) | insertion's drop placement, the same rule | element pins still end at the reference's last use |
| §23.25 parameters by their type (`layout::Pass`) | `OwnCx::pass` read by insertion and borrow | owned and lent parameters agree with every caller, as today |
| §23.25 moves at last use, construction consumes (`Moves::sole`, `last_access`, `tails`, `Em::take*`) | move rules 1 and 2 | — |
| §23.25 round two: immutable reads and loop elements borrow (`live::loans`) | borrow, rule 3's local case; a loop element of the private snapshot borrows because no user code writes the snapshot local | — |
| §23.25 balanced pairs (`opt.rs`) | pair cancellation for TIR values; `opt.rs` keeps it for generated code | — |
| §23.31 parameter and projection loans (`live::parameter_loans`, `prepare_parameter_loans`) | borrow, rule 3's parameter case | view temporaries keep their ownership flag in the emitter (erased views are an instance fact) |
| §23.31 read the selected field | the place itself: `Read [*$0 [%i] .x]` loads one field | — |
| §23.31 invariant buffer ownership; #710 loop projections (`unique_once`, `list_caches`) | hoisting, and its stable-list side table | the empty-loop entry test stays |
| §23.36 span loans (`live::span_loans`, #676) | borrow, rules 1 to 4 | the three boundary fixtures (`local-replaced-inside-the-span`, `local-moved-inside-the-span`, `local-lent-by-mut-reference`) keep their counts by rules 2 and 3 |
| #677 known-null releases | known-null rules 1 to 3 | — |
| #678 cells made before use (`Em::cell_made`) | known-null rule 5 | a suspending body's cells keep their tests, since a resume enters blocks in the middle |
| #679 matching an owned temporary moves its payload (`match_payloads`) | move rule 3 | — |
| #701 `&x` at the last access takes the cell's count (`Em::take_cell`) | move rule 4 | — |
| #710 hoisted uniqueness and buffer loads | hoisting | — |
| checker `coll_copy`, `return_copy`, `loop_binding_copy` (`ValueCopy`) | insertion's `copy` of a list or map value is the header copy (`rt:list_copy`) in stages 1 and 2, a buffer retain in stage 3 | a change through one holder never reaches another |
| checker `pins.rs` statement-granular unpin, pin flags and defer | insertion rule 6 | `collection-pinned` still panics while a reference lives |

The counts every stage-2 commit must keep are the `counts` baselines
under `crates/hd_driver/tests/cases/counts/`: a commit may only remove
count operations from them, and stage 2 ends with none above today's.

## The emitter after each stage

| Part | Stage 1 | Stage 2 | Stage 3 |
| --- | --- | --- | --- |
| layouts, field offsets (`layout.rs`) | kept | kept | the list and map kinds become ordinary structs over a buffer address |
| per-instance place resolution | new: one resolver for every place (below) | kept | kept |
| count glue: retain, release, store-leaf per layout (`count.rs` glue, `rt.rs`) | kept | kept; `copy`, `move`, `borrow`, `Drop` expand to it | kept |
| count decisions: `Moves`, `Drops`, loans, `prepare_loans`, `skip`, `moving`, `borrowed` (`count.rs`, `live.rs`) | kept, matching the new tags | deleted | — |
| references: `ref_place`, `deref_read`, `deref_write` (`emit/refs.rs`) | merged into the place resolver; erased views kept | kept | kept |
| cells (`closure.rs`) | kept; roots come from `Ref`s of locals | `cell_made` reads known-null's facts | kept (a frame slot in tier 2) |
| lists: header, literals, push, copy, snapshots, pins (`emit/list.rs`) | `list_field_set` and `ListSet` merged into the element store | pins become `Pin`; uniqueness becomes `IsUnique`; loop caches read hoisting's table | deleted: std code |
| maps (`emit/map.rs`, `map.rs`) | kept | kept | deleted: std code |
| suspension (`susp.rs`, `task.rs`) | kept | kept | kept |
| erased generic methods (`erased.rs`) | open places outlined like open instructions | kept | kept |
| Wasm peepholes, local cleanup, pair cancellation of generated code (`opt.rs`) | kept | kept | kept |

**Per-instance place resolution.** The emitter resolves a place once per
use, during its existing walk of the instance:

1. **The root** gives a base: the Wasm locals of a non-celled local
   (`Base::Vals`); the cell's payload address of a celled local
   (`Base::At`); a top-level binding's static slot or its Wasm globals;
   a value's Wasm locals.
2. **Each field** adds its offset in memory, or selects its range of
   Wasm locals in a flattened value (`path_step` returns both). A chain
   of fields folds to one constant.
3. **A dereference** loads the address the current base holds and makes
   it the base (`Base::At`); a trait value reference is the trait value
   itself, as today.
4. **An element** loads the header's buffer, checks the index against
   the length, and adds `index * size + 8` (`emit/refs.rs`'s `ELEM` root
   code, generalized).
5. **A variant** is the payload slots of the enum's layout for that
   variant, which need no branch (§23.3's slot sharing).

`Read` then loads or copies the leaves at the base, `Store` counts the
new value, stores it and releases the old one, and `Ref` produces the
base address. In an erased body, a place whose offset or result depends
on a method type parameter is an open instruction, outlined into a thunk
as an open `Field` is today.

## Stage 1 in full

Stage 1 makes places the only form for references and writes. It adds
three tags, deletes nine, removes every reference closure form, and
leaves ownership in the emitter exactly where it is. The emitted Wasm
must not change except where the plan below says it improves.

### TIR changes

- **Add** the place encoding (place word, place record, roots local,
  global and value, the frame root reserved; projections field,
  dereference and element), with `Body::decode_place(word)` decoding it
  and `TirBuilder::place(&Place) -> u32` encoding it. The operand schema
  gets a place kind, `Op::Place`, so the verifier, the wire form,
  `map_body_types`, the TIR view and the schema walkers handle place
  operands for every tag that has one. (`Body::place(i)` still decodes
  a `RefPlace` record until commit 10 deletes it.)
- **Add** `Read` (a: place with a dereference, b: mode `use`), `Store`
  (a: place, b: value) and `Ref` (a: place, b: mutable).
- **Change** `RefDyn` to own only its address-form block, and
  `RefIdentity` to a plain value instruction of its reference.
- **Delete** `RefPlace`, `RefRead`, `RefWrite`, `LocalSet`, `GlobalSet`,
  `FieldSet`, `ForRange`, `ForList`, `ForMap`; the `ListSet` and
  `ListFieldSet` intrinsics; `place_root`, `PlaceRec`,
  `Body::place_locals`, `Body::ref_operands`, `strip_closure_forms`.
- **Update** `value_operands` (a place's value root and element indices),
  the text forms (places as `[*$0 .x]`), the verifier (invariants
  1 to 3 of [Invariants](#invariants)), `wire::id_words`,
  `map_body_types` and `map_ids`. `hd_tir::ir::print` and `parse` dump
  `extra` word for word, so they round-trip place records unchanged; the
  `[*$0 .1 [%5]]` text is `Place`'s `Display` and `FromStr`, and the
  checker's TIR view (`render.rs`) prints fields by name.

### Checker changes, by function

| File | Function | Change |
| --- | --- | --- |
| `refs.rs` | `make_ref` | emits `Ref` of the place; the `get` and `set` sub-bodies, the `NewData` reference object and the identity go |
| `refs.rs` | `place_identity`, `root_base`, `new_ref_serial`, `ref_names`, `reserve_ref_serial`, `draw_ref_serial`, `any_ty` (its identity use) | deleted, with `Ck::ref_serials` and `BodyCx::ref_names` (`body.rs`) and their calls in `body.rs` and `expr.rs` |
| `refs.rs` | `emit_deref`, `deref_expr`, `deref_index_base` | `Read [*r]`; a field step on a dereference place extends the place (`field_step`), so `p.x` through `&Point` is one `Read [*$0 .x]` |
| `refs.rs` | `emit_ref_store`, `store_ref_field`, `deref_assign` | `Store [*r path] v`; `store_ref_field`'s hidden `$ref` local and nested `make_ref` go |
| `refs.rs` | `store_place`, `store_root`, `read_root` | one `Store` of the place; the `CopyData` rebuild loop goes |
| `refs.rs` | `hold` | used only where a value must outlive its expression (a new object); a dereference root takes the value root `%r` instead of a hidden local |
| `refs.rs` | `Root::DataField` | a dereference then a field on the data value |
| `refs.rs` | `ref_identity_value` | `RefIdentity %r` with no block |
| `refs.rs`, `value.rs` | the two `Place` and `Root` types | one `Place` in `refs.rs`, built by one `place_of(expr)` that `&`, `&mut`, assignment, compound assignment and receivers share |
| `value.rs` | `struct_field_assign`, `value_place`, `store_field` | build the place with `place_of`, check writability (the diagnostics stay), emit one `Store`; the walk back over emitted `Field`, `RefRead` and `ListIndex` instructions and the `unlist` calls go |
| `ir.rs` (builder) | `TirBuilder::unlist` | deleted: nothing is emitted and then taken back |
| `pins.rs` | `pin_list`, `elem_operand`, `ref_loop` | unchanged in stage 1, except that the element reference is `Ref [$pinned [%index]]`, so the hidden index local goes |
| `receiver.rs` | `auto_address`, `address_of_place`, `new_object_place` | `Ref` of the place, through `make_ref` |
| `dynref.rs` | `build_dyn_ref`, `convert_to_dyn_ref` | `RefDyn` without its closure-form block |
| `coll.rs` | `is_fresh`, `place_writable_quiet`, `mark_write_path` | match `Ref` and `Read` instead of the deleted tags |
| `using.rs`, `fnref.rs`, `pat.rs`, `facts.rs`, `literals.rs`, `expr.rs` | callers of `make_ref`, `emit_deref`, `new_object_ref` | no change: they call through the helpers above |
| `render.rs` | the TIR text view | prints places |
| `hd_structure` | generated structure bodies | `RefRead` becomes `Read`, `RefDyn` loses its block |

### Emitter changes

- **One place resolver** in `emit/refs.rs`, from `ref_place` and
  `path_step`, serving `Read`, `Store` and `Ref`, as described in
  [The emitter after each stage](#the-emitter-after-each-stage).
  `deref_read` and `deref_write` become its `Read` and `Store` through a
  dereference, keeping the erased-view paths (#619).
- **Element stores**: `list_field_set` becomes the resolver's element
  case, with the bounds check and `unique` it does today.
- **Local stores**: `Store [$l]` is today's `LocalSet` code; a store
  into a field of a non-celled struct local sets only that field's Wasm
  locals; a celled local stores at the cell plus the offset.
- **`live.rs` and `count.rs`** keep their logic and learn the new tags:
  a place's root local is an access (as a `RefPlace` root and a
  `LocalSet` are today), `Store [$l]` is treated as `LocalSet` in
  `plan`, `span_loans` and `loans`, a `Ref` rooted at a local as
  `RefPlace` with a local root, and `Read [*$p ...]` of a parameter as
  `RefRead` in `parameter_loans`.
- **`closure::cell_locals`**, `ref_roots` and `address_globals`
  (`lib.rs`) find their roots in `Ref` places.
- **`hd_mono`** (`erased.rs` `schema_values`, `open_insts`) learns the
  three tags.
- **The driver** stops calling `strip_closure_forms` in `decode`.

### What gets deleted

| Crate | Removed | Added | Mostly |
| --- | --- | --- | --- |
| `hd_tir` | 200 | 300 | closure-form stripping and reference schema out; place encoding, printer, parser and verifier in |
| `hd_check` | 900 | 250 | `make_ref`'s closure form, identity and serials, the rebuild loops, `value.rs`'s instruction walk, the second `Place` |
| `hd_wasm_linear` | 450 | 250 | `RefRead`/`RefWrite`/`FieldSet`/`ListSet`/`ListFieldSet` arms and their liveness cases; the resolver generalized |
| `hd_mono`, `hd_driver`, `hd_structure` | 60 | 40 | dead `For*` arms, the strip call |
| total | 1,610 | 840 | net about -770 lines |

### Commits

Each is about one hour, and each keeps `cargo test`, the stage fixtures
and conformance green (with `HD_CONFORMANCE_CHECKED=1` for the count
audit on the commits that touch the emitter).

1. **Place encoding.** `hd_tir`: the place word and record, `Place` with
   its decoder and encoder, its text form, `Op::Place`, wire `id_words`
   and `map_body_types` for global roots (a shared record mapped once),
   verifier invariant 1 (invariant 2's test, `Place::writable`, is
   wired to `Store` and `Ref` in commit 2), the TIR view's place
   printing, round-trip tests. No producer yet. (The dead `ForRange`,
   `ForList` and `ForMap` tags this commit was to delete went first, in
   #716.)
2. **New tags in the IR and collection.** `Read`, `Store`, `Ref` in the
   catalog with their operand schema, `value_operands`, the wire, and
   `hd_mono`'s erased-body schema. Unit tests built with `TirBuilder`.
3. **The emitter's place resolver.** `emit/refs.rs`: `Read`, `Store`
   and `Ref` for every root and projection, beside the old tags;
   `live.rs` and `count.rs` treat them as described. Unit tests in
   `hd_wasm_linear/src/tests.rs` build bodies with the new tags and
   compare the module with the old-tag body's.
4. **References without closure forms.** The checker's `make_ref` emits
   `Ref`; identity, serials and `RefRoot` go; `RefIdentity` and `RefDyn`
   lose their blocks; the driver stops stripping. TIR baselines shrink;
   wat and counts baselines stay identical.
5. **Reads through references.** `emit_deref` emits `Read`, and field
   steps through a dereference extend the place. `RefRead` has no
   producer.
6. **Writes through references.** `emit_ref_store`, `store_ref_field`
   and `deref_assign` emit `Store`. `RefWrite` has no producer.
7. **One place for every write.** `place_of` in `refs.rs` replaces
   `value.rs`'s instruction walk; struct field writes on local, global
   and data roots are one `Store`; `CopyData` remains only for
   copy-update literals; `unlist` goes.
8. **Element writes.** `xs[i] = v` and `xs[i].p.x = v` are `Store`s with
   an element projection; `ListSet` and `ListFieldSet` have no producer.
9. **Plain stores.** `LocalSet`, `GlobalSet` and `FieldSet` become
   `Store`; the emitter's helper `local_store(i)` keeps `live.rs`'s
   matches readable.
10. **Delete and document.** Remove the tags and intrinsics with no
    producer, `strip_closure_forms`, `place_root`, `PlaceRec`, the old
    resolver entry points; update `hd_tir/DESIGN.md` §4.13.11,
    `hd_wasm_linear/DESIGN.md` §23.20 and §23.32, and
    `docs/compiler/lowering-catalog.md`.

An eleventh commit is possible but not needed: stop lowering `&T` to
the `std.core.Ref[T]` data type in the checker's finish
(`lower_body_refs`, `Types::lower_refs`), so references stay
`TyData::Ref` through the backend. Without closure forms nothing needs
the data type, but the change moves instance keys and layout lookups
everywhere, so it is better done alone after stage 1.

### Acceptance cases and their expected diffs

The `tir` baselines of these cases change as shown; every other `tir`
baseline loses only closure-form instructions, hidden locals and
serial calls. The `wat` and `counts` baselines must stay identical
except where noted; any other change is a regression.

| Case (`crates/hd_check/tests/cases/known-bad-tir/`) | Today | After stage 1 |
| --- | --- | --- |
| `a-referenced-struct-field-write-rebuilds-its-value` | 46 lines: getter, setter, `CopyData`, `StrConcat`, `RefPlace`, `RefWrite` | `%0 = Store [*$0 .x] 1.0:f64` |
| `a-local-struct-field-write-rebuilds-its-value` | `LocalGet`, `LocalGet`, `CopyData`, `LocalSet` | `Store [$1 .x] 1.0:f64` |
| `a-nested-field-write-rebuilds-each-enclosing-value` | 7 instructions, two `CopyData` | `Store [$1 .body .x] 1.0:f64` |
| `a-mutable-list-reference-reads-through-a-getter` | getter call, `RefRead`, `ListLen` | `Read [*$0]`, `Intrinsic ListLen` |
| `a-mutable-generic-reference-reads-through-a-getter` | getter call, `RefRead`, `ValueCopy` | `Read [*$0]`, `Intrinsic ValueCopy` (stage 2 removes it) |
| `list-push-is-an-opaque-intrinsic` | getter call, `RefRead`, `ListPush` | `Read [*$0]`, `Intrinsic ListPush` |
| `a-list-element-reference-pins-its-buffer` | 35 instructions with two closures and an identity | `Read [*$0]`, `ListPin`, the pin flag and its `Defer`, `Ref [$2 [%5]]`, `Store [$6]`: about 18 |
| `an-element-method-call-unpins-after-the-call` | 28 instructions | `Read [*$0]`, `ListPin`, `Store [$2]`, `Ref [$2 [%5]]`, the call, `ListUnpin`: about 9 |
| `a-user-iterable-loop-uses-the-cursor-protocol` | the cursor's reference object (instructions 3 to 15) | `Ref mut [$1]` |

| Case (other crates) | Expected diff |
| --- | --- |
| `hd_check` `checker/a-compound-field-write-reads-the-field-first`, `a-struct-field-write-through-a-list-element`, `a-nested-field-write-through-a-list-element` | `Read`s and one `Store` with an element projection instead of getter calls and `ListFieldSet` |
| `hd_wasm_linear` `emitter/a-local-struct-field-write` | wat: the rebuild's leftovers go (`local.get $b.x` then `drop`, and `local.get $b.vx` then `local.set $b.vx`); one `local.set $b.x` remains |
| `hd_wasm_linear` `field-write/through-mutable-self`, `field-write/update-one-counted-field`, `emitter/a-field-write-through-a-list-element` | wat: identical (writes through references and element field writes already store one field) or fewer instructions |
| `hd_driver` `counts/field-write-counted-struct` | identical: no count operations since #656 pattern 4 was fixed |
| every other `wat`, `counts` and `layout` baseline | identical |

### Exposure along the whole path

A `Pin` marks every list on the reference's path, not only the list
whose slot it names. `&mut a[0][0]` pins the inner list and marks the
outer list unshareable, so copying `a` copies the outer buffer and, with
it, copies the pinned inner list (`r[ref.elem.copy]` applied to each
element). Today's runtime pins only the innermost list, so `b := a`
shares the outer buffer, and a write through the reference reaches `b`
(#719, found by checking an external IR proposal's §8). The insertion
pass emits one `Pin` per list on the place's projection chain, outermost
first, detaching a shared ancestor before marking it. Containment
propagates: storing an unshareable value into a fresh list or struct
makes the container unshareable. #719 fixes the runtime first; stage 2
then makes the rule explicit in the IR.

### Aliasing conformance cases

Stage 2 lands only with these runtime cases green on both engines in
checked mode. They come from the same external review (§16.3). Each
guards a rewrite the passes could get wrong:

| Case | Required result |
| --- | --- |
| Copy a list, then take `&mut` to an element and write | the copy is unchanged |
| Take `&mut` to an element, copy the list, write through the reference | the copy is unchanged |
| Replace the list while an element reference lives | the reference reads the old element |
| `&` and `&mut` to the same element | the `&` read sees the write |
| Element reference into a nested list, copy the outer list | the copy is unchanged (#719) |
| Copy a struct holding a list and a reference | the list is copied, the reference keeps its target |
| Copy at the variable's last use while an element reference lives | the copy does not see later writes through the reference (no copy-to-move) |
| Pass a pinned list by value to a function that runs a callback writing through the reference | the parameter keeps its value |
| `a[i] = f()` where `f` replaces `a` | the write goes where the language rule says; pin that rule down in the spec first |
| Copy, then write an element, in a loop where hoisting moved `IsUnique` | the copy is unchanged |

## Stage 2

Stage 2 moves every count decision into `hd_tir::own`, deletes the
emitter's decisions, and removes the checker's copies and pin
bookkeeping. The old and new paths run side by side, switched by the
driver, until the counts baselines show the new path at or below today;
then the old path is deleted.

**TIR changes.** `Read` for every read: `LocalGet`, `GlobalGet`,
`Field`, `TupleGet`, `Payload`, `Unwrap` and `ListIndex` go, with the
variant projection added. `Drop`, `IsUnique`, `Pin`; the reserved
`FlagSet`, `FlagClear`; read modes; `exit_drops`; the empty `regions`
column; `CopyData` lowered to `NewData` of field reads; `ListIndexMut`
becomes an `IsUnique` of the list and a `Read` of the element.
Invariants 3 and 4.

**Checker changes.** Reads emit `Read` (`read_local`, `field_expr`,
`pat.rs`'s payload binding, `coll.rs`'s element reads). `coll_copy`,
`return_copy`, `loop_binding_copy` and the `ValueCopy` intrinsic go.
`pins.rs` keeps the element-place and writability checks and loses its
release machinery (`PinSlot`, `pins_after`, `emit_unpin`,
`release_call_pins`, the flag locals and their `Defer`s): the checker
emits `Ref [$0 [%1]]` and nothing else. `collection_loop` emits its
snapshot for every source; move makes a fresh source's snapshot a move
(#680's rule, now a pass).

**Passes.** [The ownership passes](#the-ownership-passes), with `OwnCx` implemented
by the driver.

**Emitter changes.** `live.rs` is deleted; `count.rs` keeps its glue
(retain, release, store-leaf, exit release) and loses its decisions;
`Read` modes and `Drop` expand to glue; `IsUnique` is `rt:list_unique`
or the map's; `Pin` is today's `ListPin` without its extra count; the
loop caches read hoisting's stable-list table; `prepare_loans` keeps one
instance filter: a `borrow` of an open value in an erased body, whose
thunk may build a fresh box, is emitted as a `copy`.

**What #704 gains.** A reference made in the body is a `Ref` of a known
place: with no projection on a celled local, it is the payload start of
a whole object, and its counts can take the known-start path (§23.30)
with no tag. Only references that arrive as parameters, fields or call
results keep the page-table path. After stage 2, count how many count
operations on references remain on that path in binary_trees; the tag
is worth deciding only if they are hot.

| Crate | Removed | Added |
| --- | --- | --- |
| `hd_tir` | 100 | 1,650: six passes (about 1,300), `OwnCx`, invariant 4, the `own` printer |
| `hd_check` | 650 | 100 |
| `hd_wasm_linear` | 2,100 | 250 |
| `hd_driver` | 0 | 120: `OwnCx`, the switch, the `own` fixture view |
| total | 2,850 | 2,120 (net about -730) |

**Commits**, one hour each:

1. `Read` for plain local and global reads (`LocalGet`, `GlobalGet`).
2. `Read` for field, tuple and payload reads, with the variant
   projection (`Field`, `TupleGet`, `Payload`, `Unwrap`).
3. `Read` for element reads (`ListIndex`).
4. Ownership tags, modes, `exit_drops`, `regions`, invariants 3 and 4,
   and the `own` fixture view.
5. `hd_tir::own` skeleton, `OwnCx` in the driver, and the switch; the
   new path emits from insertion's output (copies and drops).
6. Insertion's uniqueness and pins; the intrinsics stop checking for
   themselves on the new path.
7. Move.
8. Borrow.
9. Pair cancellation.
10. Known-null.
11. Hoisting and the stable-list table.
12. The checker stops inserting copies and pin bookkeeping.
13. Parity: every counts baseline at or below today, conformance and
    the checked-mode audit green on the new path; the switch flips.
14. Delete the old path: `live.rs`, `count.rs`'s decisions, the
    loan and cache code, `ValueCopy`, `ListPin`, `ListUnpin`; update
    `hd_wasm_linear/DESIGN.md` §23.16, §23.20, §23.25, §23.31, §23.36.

## The inliner

Stage 3 moves lists, maps and strings into std and lowers every `for`
through `Iterable`. Without inlining, each element access becomes a call
and each loop step a call of `next`, so lists in loops get slower. The
inliner removes that cost.

**What it must do.** At decode, before the ownership passes, replace a
`Call` whose callee is a small std body by a copy of that body: its
locals renumbered into the caller's, its labels and sub-bodies offset,
its parameters bound with `Store`s of the arguments, each `Return` a
`Break` to a `Block` around the copy, its type parameters substituted
by the call's type arguments, which may themselves be the caller's
parameters, so a generic caller stays generic. A trait-method call
inlines only when its choice is an impl (`ChoiceKind::Impl`); a bound
or a trait value is resolved per instance and stays a call. The first
version refuses callees that suspend, register `defer` suites, create
closures or read providers, and callees over a size limit (about 40
instructions). The ownership passes then run on the combined body, so a
`ListIter.next` inlined into a loop borrows its list like a hand-written
index loop.

**What it costs.**

- **Code.** About 1,000 lines in `hd_tir` (substitution, renumbering,
  the size test) and 50 in the driver.
- **Compile time.** Only reached calls of small std bodies inline.
  Intrinsics are 2.3% of all checked instructions; at that rate json's
  reached bodies make about 300 `List`, `Map` and `string` operations.
  At about 15 copied instructions each, that adds 4,500 instructions to
  the 13,305 json emits today, about +35% to resolve and emit, partly
  offset by deleting the intrinsics' emitter arms.
- **Caching.** The inlined body is part of the caller's code, so a
  caller's emitted function depends on its callees' TIR hashes. That is
  the "dependency list" the TIR entry already plans (hd_tir/DESIGN.md
  §4.13.11, "Planned, not built"), and it matters only once a
  per-instance code cache exists.
- **Rule.** Inlining reads another body. It is a transformation, not an
  analysis whose facts a caller relies on, but it is cross-function
  work; [the last section](#where-i-would-revise-an-owner-decision) says how it
  relates to the owner's rule.

## Stage 3

Stage 3 shrinks the intrinsics to memory primitives. It starts only
after the inliner lands and the ownership passes are measured.

- **Typed memory primitives** in a privileged std module: a pointer type
  over `T` with `size_of[T]`, `read`, `write` and `offset`, counted like
  `T`, next to today's untyped `std.rt.memory` loads and stores. This is
  a language-tier intrinsic family; its signatures are plain hd
  declarations, and only their bodies are the compiler's.
- **Lists in std.** A `List[T]` is the address of a counted buffer
  holding capacity, length and the elements; the header object goes,
  saving an allocation per list and a load per access. `len` is a field
  read, an index is a bounds check and an element place, `push` and the
  other changes are std code that begins with `IsUnique`, and copy of a
  list value is a retain of its buffer. An element place then needs one
  load less, and the buffer address is an ordinary value that hoisting
  can keep in a Wasm local.
- **Maps and strings in std.** The table code of `map.rs` and
  `emit/map.rs` becomes hd over the same primitives; `StrConcat`,
  `StrEq` and `StrIndex` become std functions over bytes.
- **One `for` lowering.** Every `for` is `iter`, then a `Loop` around
  `next` and a `Match` on its option. The snapshot semantics come from
  the cursor: `ListIter` holds a copy of the list, which is a retain of
  its buffer, so a change to the original copies the buffer first, and
  the cursor's drop at the loop's end gives it back. That is exactly
  `SnapBegin` and `SnapEnd`, as ordinary values. `for p in &mut xs` is
  an iterator of element references, each pinning the list.
  `SnapBegin`, `SnapEnd`, `MapSlots`, `MapSlotNext` and `MapSlotEntry`
  go, with the checker's `collection_loop`, `range_loop` and most of
  `ref_loop`.

| Crate | Removed | Added |
| --- | --- | --- |
| `hd_wasm_linear` | 3,300: `emit/list.rs`, `emit/map.rs`, `map.rs`, `emit/iter.rs`, the list and map glue of `rt.rs` | 300: memory primitives |
| `hd_check` | 500: three loop lowerings, intrinsic plumbing | 50 |
| `hd_tir` | 50 | 1,050: the inliner |
| `lib/std` (hd) | 200 | 1,800: `List`, `Map`, string operations |

Commit plan, at plan level: the inliner (four commits), the typed
primitives (two), `List` with the buffer-address value (four, each
benchmarked), the `for` lowering (two), `Map` (three), strings (two),
deleting the intrinsics (one).

## Cost

### Measured today

Measured on 2026-10-21 at a6c3ad546's parent (origin/main 255c8b327), in
a release build, with the serial executor and a fresh in-memory cache,
so every module is checked: a cold build. Times are the minimum of six
runs; the machine was shared, with a load average of 20 to 25. The
instrumentation was scratch code (a timing example in `hd_driver` and
environment-guarded counters in the driver and the backend), not
committed.

| Stage | json (`bench/study/json.hd`) | calc (`examples/dogfood/calc.hd`) |
| --- | --- | --- |
| total | 283 ms | 178 ms |
| check: Body | 80 ms | 86 ms |
| check: FolderIface, HeaderCheck, ModulePrep, Skim, Coherence | 46 ms | 46 ms |
| ModuleFinish (TIR wire writing, diagnostics) | 20 ms | 22 ms |
| Collect | 7.9 ms | 4.2 ms |
| Link | 126 ms | 16 ms |
| Link, instrumented runs: emission of every instance | 28 ms | 5.3 ms |
| of which `live::plan` and `prepare_loans` | 4.5 ms | 1.3 ms |
| Link, instrumented runs: module finish (Wasm optimization, encoding) | 110 ms | 11 ms |
| peak memory (`/usr/bin/time -l`) | 107 MB | 56 MB |
| instances emitted / bodies decoded | 247 / 231 | 84 / 101 |

Both builds check 40 modules, std included, so they are the std-heavy
builds: 2,335 bodies for json and 2,484 for calc.

| TIR (json build, all checked bodies) | Value |
| --- | --- |
| bodies | 2,335 |
| instructions | 96,025 (41 per body) |
| `extra` words | 283,981 |
| locals | 8,320 |
| wire bytes | 4.70 MB (2,013 per body, 49 per instruction) |
| in memory, estimated (17 B per instruction, 4 per `extra` word, 13 per local) | 2.8 MB, about 3% of peak memory |
| instructions inside reference closure forms | 13,772 (14.3%) |
| of the 13,305 instructions of the 231 decoded bodies, dead after stripping | 2,765 (20.8%) |
| most frequent tags | `Block` 19,468, `LocalGet` 17,410, `Call` 7,299, `Field` 6,581, `LocalSet` 5,422, `NewVariant` 4,687 |
| intrinsics | `StrConcat` 404, `ListIndex` 397, `ListPush` 342, `ListLen` 326, `ValueCopy` 156, `SnapBegin` and `SnapEnd` 102 each, `ListSet` 80, `StrEq` 59, `MapGet` 45 |

Four more bench programs (closures, map_count, dyn_iter, nbody) show the
same shape: emission of 4.4 to 15.5 ms, of which `live::plan` and
`prepare_loans` are 1.1 to 3.2 ms (21% to 42%). Across all six programs
there are 0.8 to 1.07 instances per decoded body.

What the numbers say:

- **A warm build is collection plus link.** Checking is cached by
  module; collection and emission run on every build. On json the
  Wasm-level module finish dominates the link (about 110 of 138 ms in
  the instrumented runs), and the ownership decisions are a sixth of
  emission.
- **Per body versus per instance is not where time goes here.** These
  programs have about one instance per body, so running the passes once
  per generic body saves little time today; it saves work in generic
  std code as programs grow, and it puts the decisions where a fixture
  can show them.
- **TIR is not a memory problem**: 3% of peak. Its wire form is three
  times its memory form, because every body repeats its own type, path
  and string tables to keep its hash independent of its neighbours.
  TIR v2 does not change that.

### Estimated after each stage

| | Stage 1 | Stage 2 | Stage 3 |
| --- | --- | --- | --- |
| TIR instructions | -15% to -18%: closure forms, hidden `$ref`/`$serial` locals and their stores, rebuild chains | another -8% to -12%: `LocalGet` + `Field` chains become one `Read`; `extra` grows about 5% for place records | +20% to +40% in reached bodies after inlining; checked bodies about the same |
| TIR wire bytes | -15% (fewer instructions and fewer closure types in the per-body tables) | -5% | std grows by its new hd code |
| check (Body) | -1 to -3 ms of 80: building closure forms is a small part of checking | -1 ms (no copy insertion, no pin bookkeeping) | +5 to +10 ms cold: about 1,800 more lines of std to check |
| ModuleFinish | -3 ms (wire writing) | -1 ms | +2 ms |
| emission | unchanged | about unchanged: `live::plan` and the loans (4.5 ms on json) go, the passes (estimated 4 to 6 ms on 231 bodies) run at decode instead | +10% to +20%: more instances and inlined code |
| module finish (Wasm) | unchanged | unchanged | +5% to +10%, following code size |
| peak memory | -0.4 MB | about unchanged | +1 to +3 MB |

The run time of emitted code should not change in stage 1 (the emitter
already lowered references to addresses) except that field writes into
struct locals store one field. Stage 2 should match today's counts or
remove some: it covers today's rules in more places (bodies with
`defer`, reads through references other than parameters). Stage 3 must
be benchmarked program by program; its gains are the header allocation
and load per list, and loops whose base a pass can hoist.

### Compiler size

| Crate | Stage 1 | Stage 2 | Stage 3 |
| --- | --- | --- | --- |
| `hd_tir` | +100 | +1,550 | +1,000 |
| `hd_check` | -650 | -550 | -450 |
| `hd_wasm_linear` | -200 | -1,850 | -3,000 |
| `hd_mono`, `hd_driver`, `hd_structure` | -20 | +120 | 0 |
| Rust, net | -770 | -730 | -2,450 |
| `lib/std` (hd), net | 0 | 0 | +1,600 |

The web compiler is 5.21 MB raw against its 5.25 MB gate. Its crates
hold about 135,000 lines of Rust, and it embeds std's 700 KB of hd
source, so a Rust line costs about 33 bytes and an hd line about 40.
That gives about -25 KB after stage 1, -24 KB after stage 2, and -81 KB
of Rust plus 64 KB of std text after stage 3: about -66 KB in all, to
about 5.14 MB.

### Cognitive load

What a contributor must hold to change how references, writes or counts
are compiled:

| Today | After stage 2 |
| --- | --- |
| 64 tags; five reference tags, each owning a closure-form block that only a backend without addresses would run, and `strip_closure_forms` to remove them | 56 tags (two reserved); places, with four root kinds and four projections |
| two `Place` and `Root` types in the checker (`refs.rs`, `value.rs`), five `place_root` kinds, the `std.core.Ref[T]` object with `get`, `set`, base, serial and key | one `Place`, built by one `place_of` |
| the checker's collection-copy rules (`coll_copy`, `return_copy`, `loop_binding_copy`, `is_fresh`, `reads_fixed_local`) | none: the checker emits plain reads |
| the checker's pin release: statement-granular last use, pin flags, a `defer` per pin, call pins | none: insertion's drop at last use ends the pin |
| the emitter's count state: `Moves` (eight sets), `Drops`, `Tree`, `skip`, `moving`, `borrowed`, `view_loans`, `unique_once`, `list_caches`, and the three loan kinds with their separate soundness arguments (§23.25, §23.31, §23.36) | four read modes, three statements, six passes with one rule each, and the count glue per layout |
| why a field write through a reference differs from one through a local, and why element field writes have their own intrinsic | one `Store` of a place |

## The known-bad cases

The 18 cases of `crates/hd_check/tests/cases/known-bad-tir/` and the
#656 patterns still open:

| Case | Fixed in | New output |
| --- | --- | --- |
| a list element reference pins its buffer | stage 1 (closure form), stage 2 (pin) | `IsUnique [*$0]`, `Pin [*$0]`, `%2 = Ref [*$0 [%1]]`, `Store [$2] %2`; the reference's drop at its last use ends the pin |
| a list loop uses a snapshot and index desugaring | stage 3 | `iter` inlined: the cursor holds a copy (a retain) of the list; each step `%p = Read [$c .at]`, `Read borrow [$c .items [%p]]`, `Store [$c .at]`; the cursor's drop at the loop's end |
| a local struct field write rebuilds its value | stage 1 | `Store [$1 .x] 1.0` |
| a map loop uses slot intrinsics | stage 3 | the same `Iterable` lowering over `Map`'s std cursor |
| a mutable generic reference reads through a getter | stage 1, stage 2 | `Read copy [*$0]`; for a scalar instance the copy costs nothing |
| a mutable list reference reads through a getter | stage 1, stage 2, stage 3 | stage 2 `Read borrow [*$0]`, `ListLen`; stage 3 `Read [**$0 .len]` (the reference, then the buffer) after inlining `List.len` |
| a nested field write rebuilds each enclosing value | stage 1 | `Store [$1 .body .x] 1.0` |
| a range loop uses the cursor protocol | stage 3 | the one lowering through `RangeIter`, inlined; the bound test once (#711 handles its bounds checks) |
| a referenced struct field write rebuilds its value | stage 1 | `Store [*$0 .x] 1.0` |
| a user iterable loop uses the cursor protocol | stage 1 (closure form) | `Ref mut [$1]` for the cursor; already the one lowering |
| an element method call unpins after the call | stage 1, stage 2 | `IsUnique`, `Pin`, `Ref [*$0 [%1]]`, the call; the reference temporary's drop after the call |
| copying a struct containing a list uses `ValueCopy` | stage 2 | `Read move [$0]` at the parameter's last use: no count |
| list indexing is an opaque intrinsic | stage 2 | `Read [$0 [%1]]`: bounds check, address, load |
| list length is an opaque intrinsic | stage 3 | `Read [*$0 .len]` after inlining |
| list push is an opaque intrinsic | stage 3 | an inlined std `List.push`: `IsUnique`, a capacity test, a `Store` through an element place |
| map lookup is an opaque intrinsic | stage 3 | a std call over the table |
| string concatenation is an opaque intrinsic | stage 3 | a std call: allocate, two byte copies |
| string equality is an opaque intrinsic | stage 3 | a std call: length compare, byte compare |
| #656 pattern 3c: non-escaping `&mut` locals in a heap cell (#702) | not in tier 1 | the frame-slot root exists; deciding that a reference does not escape a call needs tier 2's signature lifetimes |
| #656 pattern 7: `list_unique` per element write in a loop | stage 2 | hoisting covers every loop it proves, not only #710's shapes |
| #656 pattern 10: a dropped chaining `&mut` result (#681) | not in tier 1 | stays a retain in the callee and a release in the caller; tier 2's `-> &{self} T` would give the callee the fact from its signature |
| #656 patterns 11 and 16: map update probes twice, `unwrap_or` is a call (#683) | stage 3 | `unwrap_or` inlines; one probe needs a std entry API, not an IR change |
| #656 pattern 12: range `for` tests its bound twice (#711) | stage 3 | the one lowering tests once; bounds-check elimination stays #711 |
| #656 pattern 13: list base and bounds per access | stage 2, stage 3 | hoisting's stable lists in stage 2; in stage 3 the buffer address is an ordinary hoisted read |

## Risks and rollback

**Stage 1.**

- *Risk:* the generalized place resolver changes emitted code for
  references to erased views (#619), whose reads and writes go through
  getter and setter slots. *Guard:* the resolver keeps `deref_read`'s
  and `deref_write`'s view branches unchanged, and the seven
  `erased-reference-*` conformance cases run in checked mode on every
  commit.
- *Risk:* `live.rs` misses an access when a tag changes, which delays a
  release (harmless) or moves a count wrongly (a leak or a use after
  free). *Guard:* commits 4 to 9 each keep the `counts` baselines
  identical, and `HD_CONFORMANCE_CHECKED=1` audits every count.
- *Rollback:* each commit is a revert. Until commit 10 the emitter still
  accepts the old tags, so reverting a checker commit alone is enough.

**Stage 2.**

- *Risk:* a pass removes a count it should not. *Guard:* the old path
  stays behind the switch until parity; invariant 4 runs in checked
  builds; the `own` fixture view makes each pass's decisions a reviewed
  diff; checked conformance audits every count.
- *Risk:* compile time grows if the passes cost more than `live::plan`.
  *Guard:* the passes run only on decoded bodies; measure the json and
  calc builds at commit 13 against the numbers above.
- *Risk:* generic bodies keep counts that a concrete instance did not
  need. *Guard:* "maybe counted" glue disappears for scalar instances;
  compare the counts baselines of `two-instances-of-one-generic`.
- *Rollback:* flip the switch back (one line) until commit 14; after
  it, revert commit 14.

**Stage 3.**

- *Risk:* lists and loops get slower before the inliner and the passes
  handle std code well. *Guard:* stage 3 starts only after the inliner,
  and each collection moves alone with bench/study before and after.
- *Risk:* compile time and std size grow (more instances, inlined code).
  *Guard:* the size of hello and the json build time are reported per
  commit.
- *Rollback:* each collection keeps its intrinsic implementation until
  its std version is measured at least as fast, then the intrinsic is
  deleted in its own commit.

## Where I would revise an owner decision

I found no strong evidence against any of the owner's decisions. Two
places need the owner's reading, because the evidence changes their
reasons or their scope, not their direction:

- **No cross-function analysis, and stage 3's inliner.** The inliner
  copies a callee's body into its caller. It produces no fact the caller
  relies on (the passes still read only signatures), but it is
  cross-function work, and it makes a caller's code depend on its
  callees' bodies. Stage 3 needs it: with lists in std, every element
  access and loop step is a call otherwise. Go, Rust and Swift all
  inline library collections across functions. Alternative: keep the
  list and string intrinsics, and stop after stage 2. Cost: stage 3's
  3,300 emitter lines and the list header stay, and so does the
  separate `for` lowering for lists.
- **Ownership passes once per generic body.** The measured programs have
  0.8 to 1.07 instances per decoded body, so this saves little compile
  time today. The decision still holds: it puts the decisions in one
  place per body, where a fixture shows them, and it scales to generic
  std code. What the measurements change is where the passes run: after
  decoding, on reached bodies, not when the module finishes
  ([where and when the passes run](#where-and-when-the-passes-run)).

Parse log: this record has no hd blocks; the `text` and `rust` blocks are
sketches and were not parsed.
