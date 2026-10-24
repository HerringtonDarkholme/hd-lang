# TIR v2: Places And Explicit Ownership

Status: detailed design, 2026-10-21, for issue #667. Stage 1 is
implemented (#718; its as-built notes are under [Commits](#commits));
stages 2 and 3 are not, and the owner approves each before its
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
- **Constructions and the rest.** `NewStruct` (named `NewData` before
  stage 1's last commit), `NewVariant`, `NewTuple`,
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
| data | `CopyData` stays only for copy-update literals, renamed `CopyStruct` (and `NewData` `NewStruct`) | `CopyStruct` gone: `NewStruct` of field reads | same |
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
   or an element in its projections (an element is a slot of the buffer
   a list header names). A value root is never written in place.
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

## What the checker lowers

The checker type-checks and emits TIR in one walk, as Zig's Sema does,
rather than building a typed tree that a later pass lowers (Rust's THIR,
Swift's SILGen). That keeps one walk and one IR. The cost of fusing is
that representation decisions leak into the type checker, so the rule
is (owner, 2026-10-21):

**The checker lowers only what needs type information.** Method and
trait resolution, coercions, match decision trees, and sugar whose
meaning depends on types (`?`, interpolation, `for` over an `Iterable`)
stay in the checker. Representation and ownership do not: copies,
drops, pins, snapshots, uniqueness, reference forms and the choice of
collection intrinsics belong to passes over TIR or to the emitter. Each
stage moves the checker toward this rule: stage 1 removes the reference
closure forms, stage 2 the copies and pin bookkeeping, stage 3 the three
`for` lowerings and the collection intrinsics. A checker change in any
stage that adds representation work is a design violation.

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

A map value is a place too (decision E, #494; `ref.elem.map`,
`types.coll.map-value.place`), with the key's entry in place of the
index: `m[k].x = v` is `%e = Intrinsic MapEntry [%m %k]` and
`Store [%m [%e] .x] v`, and `&mut m[k]` is `Ref mut [%m [%e]]`. A plain
`m[k]` read is `Read [%m [%e]]` too, and so is the receiver of
`m[k].push(v)` (`List`'s commit 2, below); `m[k] = v` stays `MapSet` (a
new key adds an entry) until stage 3 moves maps to std ([stage 2's map
value places](#map-value-places)).

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

### Data layout of the passes

The passes follow the body's own layout: flat columns indexed by `u32`
instruction and local numbers, no pointers between nodes.

- **Dense tables, reused.** Per-instruction and per-local facts are
  `Vec`s indexed by `Inst` or `LocalId`, owned by one `OwnScratch` that
  the driver keeps for the whole run and clears per body. A pass
  allocates nothing per instruction and uses no hash map: every key it
  has is a dense index.
- **Bit sets for dataflow.** Liveness, "moved" and "borrowed" are bit
  sets over locals and values, kept at block boundaries and branch
  targets only, in one flat `points x words` array. Inside a block the
  pass carries one working set forward or backward.
- **One linear walk per pass.** Each pass walks the columns once (twice
  for a fixed point across loops, bounded by the loop depth). No pass
  restarts from the top after an edit.
- **Edits in side tables.** A pass records its results in side columns
  (read modes in the data word, `exit_drops`, a `gone` bit set, an
  insertion list of `(before Inst, statement)`), and the body is
  rebuilt once at the end of the pipeline with every `Drop`, `IsUnique`
  and `Pin` spliced in. Instructions are never inserted one at a time.
- **Measured.** Each stage 2 commit reports time and allocations per
  pass on the bench/study programs and on std, beside the count and
  instruction totals. The whole pipeline should stay well under the
  checker's own time per body (about 1.2 us per TIR instruction today).

These are the rules the compile-time audit of 2026-10-21 found broken
elsewhere: the backend's peepholes restart and clone after each edit,
and its instruction type is 288 bytes, which makes `Module::finish`
about half data movement on json.

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
7. **Header copies** (commit 13a). A list or map value that a store, a
   result, an argument or a capture would leave held by two places, one
   of which a change may reach in place, gets a `ValueCopy`: the
   checker's collection copies (`coll.rs`), decided from the changes
   rules 5 and 6 find (commit 13a's note has the rule).

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
| checker `coll_copy`, `return_copy`, `loop_binding_copy` (`ValueCopy`) | insertion's rule 7 (`own/copies.rs`, commit 13a): a `ValueCopy` where a header that may change in place would be held by two places, the checker's rule decided by the pass; a buffer retain in stage 3 | a change through one holder never reaches another |
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
| lists: header, literals, push, copy, snapshots, pins (`emit/list.rs`) | `list_field_set` and `ListSet` merged into the element store | pins become `Pin`; uniqueness becomes `IsUnique`; loop caches read hoisting's table | the header and its copy go; element places, `push` and `len` stay lowered over the buffer address (option B); growth, the unique copy and truncation are std code |
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
   As built: `ReadMode` names the four modes; verifier invariant 2
   (`hd_tir`'s 21) and invariant 4 for a `Store` into a bare local; in
   an erased body a `Ref`, and a `Read` or `Store` of a bare local, stay
   in the body as `RefPlace`, `LocalGet` and `LocalSet` do, an open
   `Read` or `Store` rooted at a value is outlined, and one rooted at a
   local or global is refused (`NotImplemented`) until the emitter can
   pass its root to a thunk.
3. **The emitter's place resolver.** `emit/refs.rs`: `Read`, `Store`
   and `Ref` for every root and projection, beside the old tags;
   `live.rs` and `count.rs` treat them as described. Unit tests in
   `hd_wasm_linear/src/tests.rs` build bodies with the new tags and
   compare the module with the old-tag body's.
   As built: the resolver reads a place as a root, a head (none, a
   dereference, a data object's field, or an element) and a struct
   field path, and `RefPlace`, `RefRead` and `RefWrite` decode into the
   same form, so every existing baseline runs through it. `Read` lowers
   a place through a dereference, whole or with a field path that loads
   only that part. `Store` lowers a store through a dereference (a field
   path goes through a field view, made and released for the write,
   only when the reference is a view), into a data object's field, and
   into a whole local or top-level binding. A struct field of a local or
   top-level binding and a list element stay unsupported until commits
   7 and 8 bring their producers, since today's emitter has no
   one-instruction form of them to share. An element step does not
   check its index again: `ListPin` checked it. `live.rs` names the
   shared tests (`local_store`, `place_local`, `ref_local`, `ref_locals`,
   `read_through`), which `count.rs`, `list.rs`, `closure.rs`, `emit.rs`
   and `lib.rs` use for both tag sets; `parameter_loans` also takes the
   parameters that hold references, since a data object's field is no
   referent of one. In an erased body a `Read` or `Store` of a whole
   referent (`[*r]`) stays, as `RefRead` and `RefWrite` do. Module
   comparisons in `tests.rs` would need a whole program's tables, so a
   `live.rs` unit test plans one body with each tag set instead, and a
   local experiment that rewrote the old tags into `Ref`, `Read` and
   `Store` at decode gave byte-identical modules for every runtime
   conformance case (`HD_CONFORMANCE_WASM_DIGEST=check`).
4. **References without closure forms.** The checker's `make_ref` emits
   `Ref`; identity, serials and `RefRoot` go; `RefIdentity` and `RefDyn`
   lose their blocks; the driver stops stripping. TIR baselines shrink;
   wat and counts baselines stay identical.
   As built: `RefRead` and `RefWrite` lose their closure forms here too
   (their `get` and `set` closures no longer exist, and the driver no
   longer strips), so commits 5 and 6 only change the tags. `make_ref`
   reads an element's index from its hidden local with a `LocalGet`
   for the element projection (the hidden local goes when commit 8
   rewrites `pins.rs`). The closure form's reads also checked the
   locals and top-level binding the place starts from and set their
   flags; `make_ref` keeps those effects (`Ck::note_read`, the part of
   `read_local` that makes no value, and `Ck::global_read`, the part of
   `global_get`), including the `CAPTURED` flag and a `&mut` local's
   `CAPTURED_ASSIGNED`, because capture modes, collection copies and
   the backend's loans read them; stage 2 revisits them. `store_place`
   and `store_root` go here, since the `set` closure was their last
   user. The seeded `std.core.RefRoot` and `std.core.ref_serial` go
   with their host ABI row and the emitter's lowering. The checker's
   `later_arg_change` counted the getter call of a read through a
   reference as a possible change of an earlier argument's local; it
   now counts `RefWrite` and `Store` instead, so a read no longer
   forces a list copy (`Sha256.finish_plain`). Across the runtime
   conformance modules the code then differs only in the serial
   locals' frame and closure-environment slots, that copy, and the
   order and numbering of functions, types and static data.
5. **Reads through references.** `emit_deref` emits `Read`, and field
   steps through a dereference extend the place. `RefRead` has no
   producer.
   As built: the root is the reference value, `Read [*%r]`, where `%r`
   is the `LocalGet` of `$0` while stage 1 keeps `LocalGet` (stage 2
   folds it into the root), so `p.x` through `&Point` is
   `%0 = LocalGet $0`, `%1 = Read [*%0 .x]`. The checker emits the
   whole read first; `Ck::field_read`, which `field_expr`, embedded
   field promotion, `read_place` and a compound field write's read
   use, extends it with the struct field when the read is the body's
   last instruction (nothing uses it yet) and rewrites its place record
   in place; any other base keeps a `Field`. A tuple element, a data
   object's field and a second dereference stay their own
   instructions. `Ck::read_through` decodes such a read for the
   walkers that recognized `RefRead` (`value_place`, `arg_root`,
   `place_writable_quiet`, `readonly_ref_root`); `value_place` turns
   its field path into the written place's prefix. `hd_structure`'s
   generated `dbg` bodies read `[*%r]` too. In the emitter, a match
   over `Read [*%r .f]` through a borrowed reference borrows the field
   for the match span as it did a `Field` of a borrowed whole read
   (`count.rs`), and `field_spans` sees `Ref` roots as `RefPlace`
   ones. Across the runtime conformance modules a read through a
   reference with two or more field steps loads only its last field
   instead of the first field's whole value; one function,
   `std.regex.literal_near`, gains a retain and release of the string
   `vm.filter.literal`, because the count now sits inside the
   physical branch's `block`, where `opt::count_pairs` stops, where
   before a borrowed `filter` let a `Field` outside the block count
   the string next to its release. Other differences are suspending
   bodies whose releases move with instruction numbering, since
   `live::uses` reads an `Await`'s callee record as values.
6. **Writes through references.** `emit_ref_store`, `store_ref_field`
   and `deref_assign` emit `Store`. `RefWrite` has no producer.
   As built: the root is the reference value, as for reads: `*r = v` is
   `Store [*%r] v` and `body.x = v` through `&mut Body` is
   `Store [*%0 .x] v`, after the RHS, with no hidden `$ref` local, no
   field reference and no `make_ref`. `deref_assign` reaches it through
   `emit_ref_store`, and `value.rs`'s field writes through
   `store_ref_field`, which now takes the reference value and the field
   path only. The emitter writes a field path in place
   (`deref_field_write`), so the projected field reference, its
   retain-free tee and zeroing go: `field-write/through-mutable-self`,
   `field-write/update-one-counted-field` and
   `allocation/mutable-local-of-parameter` lose instructions. Through a
   representation view it projects with the borrowed `RefProject`: only
   a view reaches that helper there, and a view holds its source either
   way, so the counting helper only added a retain that never runs.
   #717's
   `field_spans` rejected `RefWrite` but not `Store` in a match span; it
   now treats a `Store` as `LocalSet` when it replaces a whole local and
   as `RefWrite` otherwise, so
   `field-write/match-field-store-keeps-count` keeps its counts.
7. **One place for every write.** `place_of` in `refs.rs` replaces
   `value.rs`'s instruction walk; struct field writes on local, global
   and data roots are one `Store`; `CopyData` remains only for
   copy-update literals; `unlist` goes.
   As built: `place_of` is the builder `&` already used (`operand`),
   with a `PlaceUse`: `Ref` for `&`, `&mut` and automatic addresses,
   `Write` for a write's target. Its roots are `Ref`'s and `Store`'s:
   a local and a top-level binding are not read; a reference and a data
   value are value roots, `[*%r .x]` and `[*%o .k .x]`, so `hold`'s
   hidden local goes for them in `&` and receivers too, and `Ref`s of
   such places lose a `LocalSet` and its count. `Write` differs only
   where a write's place is no reference's: a list element is named
   where it is (`Root::Slot`, no pin), a map value is a read that a
   field write reports (`types.coll.map-value`), and any other field
   or index on a value reads as an expression does
   (`field_of_value`, `index_of_value`, split out of `field_expr` and
   `index_expr`). A local root is read for its flags and checks
   (`note_read`), as the base's read was. The emitter stores a struct
   field of a local in its Wasm locals or its cell, of a top-level
   binding in its static slot or Wasm globals, and of a data object's
   field at its offset, each counting only that field (and copying a
   collection a place holds, as `CopyData`'s slot did); a cell an
   erased body converts is refused. `live.rs` gains `local_part_store`
   (a `LocalSet`, or a `Store` into a local or a field of it), which
   span loans, `loans`, field spans, `written` parameters, list aliases
   and the buffer-sharing proof read where they read `LocalSet`, and
   `parameter_loans` borrows a `Ref [*%r ..]` whose `%r` reads a
   reference parameter, as it borrowed one rooted at the hidden local.
   An element field write stays `ListFieldSet` until commit 8.
8. **Element writes.** `xs[i] = v` and `xs[i].p.x = v` are `Store`s with
   an element projection; `ListSet` and `ListFieldSet` have no producer.
   As built: the root is the list value the checker evaluated before
   the right-hand side, as `ListSet` took it: `xs[i] = v` is
   `Store [%l [%i]] v` and `bodies[i].x = v` through `&mut List[Body]`
   is `%1 = Read [*%0]`, `Store [%1 [%2] .x] %15`, so the header the
   write goes through is the one the target named, and loop caches,
   uniqueness and pins find the list as they did. Invariant 2 counts an
   element as it counts a dereference: a list header names its buffer,
   so the store changes the buffer, never the value `%l`. Stage 2 folds
   the read into the place (`[*$0 [%2] .x]`), as it folds `LocalGet`
   into reads. The emitter's element store (`elem_store`) is
   `list_set` and `list_field_set` made one: the bounds check, the
   uniqueness check, the count of the new value before the old one's
   release, `collection-pinned` only for a length change. `list.rs`
   (loop caches and their safety proof, the buffer-sharing proof) and
   `parameter_loans` treat a `Store` with an element projection as
   they treated those intrinsics (`live::elem_store_list`). `&xs[i]` is
   `Ref [$pinned [%i]]` with the index value, so the hidden index
   local goes. `index_kind` still names `ListSet` as a list's set
   operation, which nothing emits; commit 10 removes it with the
   intrinsics.
9. **Plain stores.** `LocalSet`, `GlobalSet` and `FieldSet` become
   `Store`; the emitter's helper `local_store(i)` keeps `live.rs`'s
   matches readable.
   As built: `TirSink::set` emits `Store [$l] v`, a top-level binding's
   store is `Store [@g] v` (`Ck::store_global`) and a data value's field
   store `Store [*%o .k] v`. The checker's finish settles a global
   root's type as it settles the types in records, since an init stores
   its bindings before their types are known. `Body::stored_local(i)`
   names the local a `Store` writes in place (all of it, or a struct
   field path) for the readers outside the emitter: `check_receiver`,
   `later_arg_change` (a store into another local or a field of one no
   longer forces an argument copy, as `LocalSet` did not),
   `hd_mono`'s default specialization and `hd_opt`'s `value_flow`,
   which count a field store as a write with no known value. `live.rs`
   `field_spans` and `list.rs` `range_indices` read stores through the
   same helpers. In an erased body, an open store into a field of a
   local is outlined (`hd_mono::outlined_store_local`): the thunk takes
   the local's value as a capture, stores the field at concrete types
   and returns the new value, which the body stores into the whole local
   (or its cell); an open store into a top-level binding is outlined as
   `GlobalSet` was. In a concrete instance, a field store into a cell
   that keeps its erased view copies the value out, changes the field
   and stores it back. Both were refused since commit 7
   (`runtime/valid/erased-local-field-store`). Every conformance module
   is byte-identical to commit 8's; TIR baselines print `Store [$l]`.
10. **Delete and document.** Remove the tags and intrinsics with no
    producer, `strip_closure_forms`, `place_root`, `PlaceRec`, the old
    resolver entry points; update `hd_tir/DESIGN.md` §4.13.11,
    `hd_wasm_linear/DESIGN.md` §23.20 and §23.32, and
    `docs/compiler/lowering-catalog.md`.
    As built: `LocalSet`, `GlobalSet`, `FieldSet`, `RefPlace`,
    `RefRead` and `RefWrite` go, with `place_root`, `PlaceRec`,
    `Body::place`, `Body::place_locals`, `Body::ref_operands` and
    `strip_closure_forms`; `RefDyn` and `RefIdentity` lose their unused
    block operand. With the three `For*` tags #716 deleted, that is the
    nine of [TIR changes](#tir-changes); the catalog has 58 tags. The
    `ListSet` and `ListFieldSet` intrinsics go (22 remain), and
    `index_kind` names only a collection's read operation: a list
    element is stored as a place, a map value with `MapSet`. `NewData`
    is `NewStruct` (it also builds a compiler-supplied data object) and
    `CopyData`, which copy-update literals still produce, `CopyStruct`.
    The emitter's resolver reads only places: its `Spot` comes from a
    place word, an element index is always a value, and `Store` no
    longer has a reference-record form. `live.rs`'s `local_store` and
    `local_part_store` read `Body::stored_local`. `lowering-catalog.md`
    named none of the deleted forms. The `tir` baselines print
    `NewStruct` and `CopyStruct`; every module is identical to commit
    9's. Stage 1 is done.

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
column; `CopyStruct` lowered to `NewStruct` of field reads; `ListIndexMut`
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
   As built: `TirSink::get` emits `Read [$l]` (the bare-local place
   word, no record) and `TirBuilder::read_global` emits `Read [@g]`,
   both mode `use`; the two tags and `Op::Local` go (56 tags).
   `Body::local_read` and `Body::global_read` name what such a read
   reads, and every reader that matched the old tags matches them
   instead: the checker's place walkers (`coll.rs`, `access.rs`, the
   fallback column), `hd_mono`'s default specialization and erased
   bodies (an open `Read [@g]` is outlined as `GlobalGet` was),
   `hd_opt`'s `value_flow`, and the emitter (`Em::local_read`,
   `Em::global_read`, `live.rs`, `count.rs`, `list.rs`). Where a reader
   treated every `Read` as a read through a reference (unused reads,
   list headers, a field of a borrowed parameter read), a plain read is
   kept out, as the old tags were. A dereference rooted at a local's
   read stays `Read [*%r]` with `%r = Read [$l]`; folding it into
   `[*$l]` changes what the loans key on and waits for the passes.
   Every conformance module is byte-identical to the stage 1 result
   (`HD_CONFORMANCE_WASM_DIGEST=check`); only `tir` baselines change.
2. `Read` for field, tuple and payload reads, with the variant
   projection (`Field`, `TupleGet`, `Payload`, `Unwrap`).
   As built: one read for each old instruction, rooted at the value it
   projected from: a struct, newtype or tuple field is `Read [%v .k]`
   (`TirBuilder::field`), a compiler-supplied data object's field
   `Read [*%o .k]` (`TirBuilder::data_field`, the place `Store` already
   wrote), and a payload field `Read [%v as .V .k]`
   (`TirBuilder::payload`; an `Option`'s `.Some` value is
   `[%v as .1 .0]`). The checker picks the data form by the base's type
   (`Ck::field_value`, `is_data_object_ty`), and so does `hd_structure`.
   The four tags go (52 tags). `Body::read_step` decodes such a read's
   one step without allocating. A place cannot tell a struct field from
   a tuple element, or a data object's field from a field through a
   reference (`[*%r .k]`), so the readers that told the old tags apart
   ask the base's type: the checker through `Ck::projection`, the
   emitter through one per-body table, `Em::projs` (`live::Projected`),
   which `live.rs` reads where it has no types. The emitter lowers each
   to the old code (`Em::projection_read`). A read rooted at a local or
   a reference value is not folded (`[$l .x]`, `[*%r .x .y]`): the
   loans and moves key on the plain read, so folding waits for the
   passes. A field read now has a record in `extra` (4 words; 5 for a
   data object's field or a payload field, where `Payload` had 3).
   Every conformance module is byte-identical to commit 1's; only `tir`
   baselines change.
3. `Read` for element reads (`ListIndex`).
   As built: every element read is `Read [%h [%i]]`
   (`TirBuilder::element`), rooted at the list header value read before
   it, as element stores already are: an index expression
   (`Ck::index_read`), a compound element assignment, the list `for`
   loop's element, the read of an element place (`Root::Slot`,
   `Root::Elem`) and a list newtype conversion's loop.
   `IntrinsicOp::ListIndex` goes (21 intrinsics), and the checker names a
   built-in index by its own `Indexed` (list, map, string) instead.
   `Body::element_read` decodes such a read without allocating. Every
   reader that matched the intrinsic matches the read: the checker's place
   walkers step from an element to its list (`Ck::element_of`), and
   `mark_write_path` turns an element read that a change goes through into
   `ListIndexMut` in place, rewriting its five-word place record as the
   intrinsic's three-word record, so no new record is made that a
   rollback could drop. `ListIndexMut` stays until insertion's `IsUnique`
   replaces its uniqueness check (commits 6 and 12). The emitter lowers the
   read as before (`list_index`), and `live.rs`, `count.rs` and `list.rs`
   treat it where they treated the intrinsic: a loop element's loan
   (`loans::source`, `loop_read`, `prepare_loans`), the range proof and
   the loop caches, which find it by its place, and the list header
   loans and the list alias walk, which keep it out of the arms for
   reads through a reference. **Not rooted at the local**: `xs[i]` on a
   local is `Read [$l]` then `Read [%h [%i]]`, not `Read [$l [%i]]`;
   through `&List` it is `Read [*%r]` then `Read [%h [%i]]`, not
   `[*$r [%i]]`; element stores keep their separately read header too.
   The emitter's moves and loans key on that header read (the loop
   snapshot's source in `live::loans`, the header local's
   `Moves::last_access`, `list_root`, the range proof), and `list_index`
   takes the header as a value. A rooted read drops the header read and
   changes what those see, so its modules would not be byte-identical.
   Rooting waits with the other place folding for commit 12, when the
   checker stops inserting copies, and commit 13 checks it. Every
   conformance module is byte-identical to commit 2's, dev and optimized
   pipelines; only six `tir` baselines change, one line each.
4. Ownership tags, modes, `exit_drops`, `regions`, invariants 3 and 4,
   and the `own` fixture view.
   As built: `Drop`, `IsUnique`, `Pin` and the reserved `FlagSet` and
   `FlagClear`, each a place in a and `void` (57 tags; 56 once
   `CopyStruct` goes). Read modes were already `Read`'s data word b
   (`ReadMode`), where the checker writes `use`. `Body::exit_drops`
   (rows of a sub-body and a local, sorted) and `Body::regions` (empty)
   are columns that the printer and parser carry and that the wire
   refuses, as it refuses the ownership statements, so pass output never
   reaches a `check` entry; `hd_check::render` prints a body's exit set.
   Invariant 3 is invariant 22 of `verify`, which every body the checker
   finishes passes. Invariant 4 is invariant 23 of `verify_owned`
   (`crates/hd_tir/src/ir/owned.rs`), the verifier of a body after the
   passes: every read's mode decided; no read after a move or drop of an
   overlapping place on any path (a forward walk of the structured
   control flow, joined by union, around loops to a fixed point); no
   borrow live across an overlapping write or a suspension point (its
   span an instruction range through block tails, coercions and `Break`
   values, widened to a loop that holds a use but not the read); an exit
   set in order; nothing tier 2 reserves. The walks keep dense tables and
   bit sets, no hash maps. **Not yet**: invariant 4's first clause, that
   every owned value has one consumer or one `Drop` on each path. Whether
   a use consumes depends on the callee's parameter modes (`OwnCx::pass`)
   and on whether its type holds a counted leaf (`OwnCx::counted`), and
   which tags consume is what insertion defines; both arrive in commit 5,
   which adds the clause and runs `verify_owned` after the passes in
   checked and debug builds, its first caller outside the tests. The
   `own` view (`@dump: own`, `hd_testkit`'s `stages/own.rs`) renders each
   body collection reads (`Run::body_of`, `Output::own_text`) and keeps
   the instructions inside the subject by each instruction's own syntax
   node (`Positions::node_span`), so it stays right when the passes
   renumber a body. Until commit 5 runs the passes it shows the decoded
   checker TIR, every mode `use`. Its two cases
   (`crates/hd_driver/tests/cases/own/`) dump `own` beside `counts`. What
   lowers the statements comes with the path that emits from them in
   commit 5: the emitter, where they are `unsupported` until then, and
   hd_mono's erased-body outlining. No emitted module changes.
5. `hd_tir::own` skeleton, `OwnCx` in the driver, and the switch; the
   new path emits from insertion's output (copies and drops).
   As built: `hd_tir::own` runs, per body, place folding then
   insertion, and splices every new instruction in at once
   (`own/splice.rs`): each before the first instruction of the
   statement it precedes, the whole body renumbered by the operand
   schema. `OwnCx` has `counted` (no, yes, or maybe for a type naming a
   parameter or an associated type), `pass` and `pass_value` (`Owned`
   only for a parameter declared a value type with a counted leaf;
   `Lent` for a reference, a type parameter, an associated type, a
   newtype over a parameter, and every argument of a suspending
   callee), and `may_write`. The driver's `Env` implements it from
   signatures and layouts, remembered per run, never reading another
   body; the declared types are not substituted, since that would read
   a default body's generics. The switch is `Packages::own_passes`
   (`HD_CONFORMANCE_OWN=1`; the `own` view sets it). With it on,
   `Run::body_of` returns a decoded body after the passes (a second
   cell beside the checker's, its TIR hash salted), supplied and `dbg`
   bodies go through them too, checked and debug builds run
   `verify_owned`, and the backend emits from them (`Input::owned`).
   Off, the default, every conformance module is byte-identical to the
   previous commit's, dev and optimized pipelines.
   **Insertion as built.** Every `use` read copies. A value whose type
   may hold a counted leaf is consumed by its one use when that use
   consumes it as a statement of the value's own block, or as that
   block's tail which the block's owner takes; any other consumer gets
   a `Read copy [%v]` just before it, and the value a `Drop` after the
   last-use statement of its home block (its own block, or a `Match`'s
   block for a value a decision makes and an arm reads). An owned local
   (a counted type, or one a reference or a shared capture reaches, so
   a cell may hold it) gets a `Drop` after its last access in the
   innermost block holding all its accesses, the root's for a parameter
   or a capture, and a row in its sub-body's exit set. A `Break`,
   `Continue` or `ToArm` drops what it leaves that may still hold a
   count; a `Return` the values, the exit set the locals; a place a
   crossed cleanup scope's suite reads is left for the suite. The uses
   that consume (`own::operands`, `own::consumes`): a stored value, a
   construction's part, a `Return` or `Break` value, a call argument
   whose parameter is owned (not one a default argument's body reads
   too, invariant 15), and a block tail that an `If`, a `Match` arm, a
   switch with a value, a `With`, a cleanup scope's body, a `RefDyn`
   or a sub-body's root takes. `verify_owned` gains invariant 4's first
   clause (`Balance`), and three corrections found on real bodies: a
   `Drop` of a whole local empties only the local (a reference keeps its
   cell), a value computed again refills its places, and a cleanup
   scope's suites run from the states at the body's exits.
   **Place folding** is not in the plan above: `%v = Read [$l]`
   followed at once by its one use `Read [%v .k]` becomes
   `Read [$l .k]`. Without it insertion copies a whole local for each
   field read: in `wide-value-field-reads` (a 1,650-value struct) one
   function passed V8's 7.6 MB limit. It folds struct, tuple and
   newtype fields of a local whose type names no parameter; elements,
   payloads and dereferences stay as the checker writes them until
   commit 12 roots every place in the checker, which leaves this step
   nothing to fold, so it goes.
   **The new path** (`hd_wasm_linear/DESIGN.md` §23.16, "Owned
   bodies") lowers the modes, the `Drop`s and the exit set, and moves a
   value into the consumer `own::consumers` names; it runs none of
   `live.rs`, the loans or the moves. Its own glue covers what a generic
   body cannot see: a consumer whose lowering cannot move a count in
   takes one and the value is released after it, a lent argument a
   callee instance owns takes one, a parameter its caller lends takes
   one on entry, and a cancelled suspension point releases what the
   body holds there. In an erased body a `Drop` and a value copy stay
   in the body (hd_mono): an open value's view is one counted address.
   On the new path, all 4,836 driver conformance cases pass, checked
   and not, dev and optimized pipelines, and so do the std runtime's hd
   tests but one count bound: in checked builds `count_tests.hd`'s
   "immutable loop elements add no per-element counts" counts 20,000
   retains over its 1,000 rounds, against at most 2,000, since that bound
   is the old path's loop-element loans (§23.25), which the borrow pass
   (commit 8) replaces. Counts are higher than the old path's, as
   insertion alone is: over the 629 conformance programs with a `main`,
   51,245 retain and 79,135 release sites against 25,573 and 50,018
   (module bytes +8%); over the bench/study programs 2,380 and 3,079
   against 760 and 1,261.
   **Left for later commits**, each removed by its own writer: a `move`
   or `borrow` read lowers only for a plain local read (commits 7 and
   8); a `Drop` of a projection does not lower (commit 7's partial
   moves); `IsUnique` and `Pin` lower only for a local or a value
   (commit 6, which writes them); the intrinsics still check uniqueness
   and the checker still copies collections (`ValueCopy`) and pins
   (commits 6 and 12).
6. Insertion's uniqueness and pins; the intrinsics stop checking for
   themselves on the new path.
   As built: insertion's rules 5 and 6 (`own/insert.rs`). An `IsUnique`
   stands before every change of a collection place: a `Store` through
   an element projection (one per list on its path, outermost first),
   and `ListPush`, `MapSet`, `MapRemove`, `MapIndexMut` and the
   checker's `ListPin`, of their collection operand. Each checker
   `ListIndexMut [%xs %i]` becomes `Read [%xs [%i]]` after an
   `IsUnique [%xs]`, so no `ListIndexMut` reaches the new path's
   emitter. A `Ref` of an element place gets an `IsUnique` and a `Pin`
   of each list on its path, outermost first; the reference holds a
   count of the buffer, so its `Drop` from rule 2 or 3 ends the pin.
   The backend lowers both for a local or value root followed by struct
   fields and elements, bounds-checked (`Em::place_header`); that is
   every place insertion names while the checker reads a collection
   before changing it through a reference. In an owned body and its
   erased thunks (`Em::unique_explicit`) the element store, `MapSet`,
   `MapRemove`, `MapIndexMut` and `ListPin` check no uniqueness, and
   `ListPush` only grows a full buffer (`rt:list_grow`). `IsUnique` and
   `Pin` are value operands (`Body::value_operands`), so an erased body
   outlines one over an open collection. An `IsUnique` of a map picks
   `rt:map_unique` by the map's two type arguments; commit 5's lowering,
   which never ran, gave a map `rt:list_unique`. Old path: every
   conformance module byte-identical, dev and optimized pipelines. New
   path: all conformance cases pass, dev and optimized, checked and
   not, but commit 5's one checked count bound in `count_tests.hd`.
   Over the bench/study programs the passes write 220 `IsUnique`s, one
   for each change the old path's intrinsics checked; as Wasm call sites
   `rt:list_unique` goes from 19 to 45 and `rt:map_unique` stays at 6,
   with 21 `rt:list_grow`s, since the old path's loop checks run once
   per loop (#710's `unique_once` and caches) and insertion's run per
   change until hoisting (commit 11). No program there takes an element
   reference, so neither path pins. A new runtime case,
   `element-reference-aliasing.hd`, covers the aliasing cases above that
   no case did (1, 4, 6, 7, 8 and a loop form of 10).
   **Left for later commits.** The checker still writes `ListIndexMut`
   by rewriting an element read in place (`mark_write_path`), since it
   alone sees every change a read leads to: `xs[i].bump()` for a
   `mut self` method is a plain `Call` with an owned argument, which no
   pass can tell from a copy, and the old path, which must stay
   byte-identical, reads the mark for its uniqueness. Removing the
   rewrite needs the passes to know such receivers, a signature fact
   `OwnCx` does not have (an owner decision), and the old path gone
   (commit 14). The checker's `ListPin` still makes the pin of a `for`
   over element references and its `ListUnpin`s end it, so an element
   reference is made unique and pinned twice; the second `IsUnique` and
   `Pin` change nothing at run time, and commit 12 deletes the first.
   Two checks stay outside the passes: std's `list_truncate` (`pop`,
   `insert`, `remove_at`, `clear`) changes the list behind a reference
   inside the callee, where no pass sees the change, until stage 3 makes
   it std code; and a loop cache's check before the loop (§23.31) guards
   the buffer address the loop keeps, until hoisting's table replaces
   the caches (commit 11). Aliasing case 9 (`a[i] = f()` where `f` replaces `a`) waits
   for its spec rule.
7. Move.
   As built: `own/moves.rs`, after insertion. One backward liveness walk
   per sub-body over the structured control flow, one bit per candidate
   local and temporary, states at block boundaries, labels and loop heads
   in one reused arena, each loop to a fixed point, decides every `copy`
   read: when no path reads its root again, the read moves. A `Drop` is
   no read (a moved place is zero, so a drop of it releases nothing), and
   a `Store` into all of a local ends its life. The candidates are the
   counted locals that no closure shares, no cleanup suite touches and no
   `borrow` reads, and the tracked temporaries a `copy` reads in place.
   - **Locals** (rule 1): `[$l]` and a field place of one (`[$l .a]`)
     move, and the local's `Drop` right after the read goes. This covers
     more than the plan's statement-level rule: a last read in one
     branch, a pattern binding read in its arm, and a read in a loop that
     stores the local again each round move too; their later drops stay,
     releasing nothing, until known-null (commit 10) deletes them.
   - **Temporaries** (rule 2): a whole `copy [%v]` moves when `%v`'s
     drop follows it in its block, and the drop goes. The copy becomes a
     `move` of the temporary rather than the temporary itself: the counts
     are the same, and `own::consumers` keeps its one-use rule.
   - **Partial moves** (rule 3): a `copy` of a field or payload field of a
     local or a temporary at the base's last use moves. When the base's
     drop follows in the block, it becomes drops of the base's other
     counted fields, or nothing. A temporary whose last uses are inside
     the `If`, switch or match right before its drop has the drop sunk
     into that statement's paths (`moves::sink`): at the end of each path
     that completes, and before each `ToArm` of a decision tree, a drop of
     what that path's last move left, or of the whole temporary where no
     move took a part; a whole copy at a path's end moves, and a match's
     drops of the temporary before jumps in its arms go. So `?` and a
     match on a call's result lose the temporary's drop. Sinking is done
     only where it adds no drop, and never through a guard, a loop or a
     nested match. Splitting a drop needs the base laid out in place in
     every instance, a new `OwnCx` fact, `parts`
     (`hd_wasm_linear::in_place_parts`: a newtype's field, a struct's or a
     tuple's fields, a non-recursive enum's payload). A type naming a
     parameter has none, since an erased instance holds its value as one
     counted view, so a generic base keeps its whole drop, which releases
     what the move left.
   - **References** (rule 4): a `Ref` of a whole celled local at its last
     access takes the local's count of the cell (`ref_word::TAKES`, a bit
     of `Ref`'s word, which invariant 22 refuses before the passes), and
     the local's drop right after it goes; not in a suspending body.
   - **The backend** lowers a `move` of every place a move names: a local
     or a whole value, or a part of either laid out in its locals
     (`Em::part_slots`), zeroing what it took only for a counted
     instance. A part at no such place at an instance (a boxed enum's
     payload, an open value's part in a thunk) is read as a copy, and the
     whole drop the pass kept for such a base releases it. A `Drop` of a
     part releases and zeroes that part (`Em::own_drop_part`). A taking
     `Ref` is the cell's address with the local's count; where the
     reference is no cell's address (a trait value's, an open cell's
     view) it counts as before and the local is released after it.
   - **`verify_owned`**: a `move` of a whole value consumes it; a move or
     a drop of a part takes that part, after which only more parts or a
     drop of the rest may follow, and the value stays owned while the
     rest holds a count (`OwnCx::parts` again); a taking `Ref` empties its
     local.
   - **Splicing**: `splice::apply` also deletes statements, and move
     rebuilds the body once more after insertion's rebuild. The plan's
     single rebuild at the end of the pipeline would make move read
     insertion's drops from side tables; a rebuild per pass that edits
     costs one linear pass and keeps each pass a walk of a real body.
   Old path: every conformance module byte-identical to main, dev and
   optimized pipelines. New path: all conformance cases pass, dev and
   optimized, checked and not, but commit 5's checked count bound in
   `count_tests.hd`, now 12,000 retains against 20,000 before move.
   Static count sites over the 768 runtime conformance programs with a
   `main`: 52,747 retains and 85,119 releases with insertion alone,
   39,800 and 77,230 with move, against the old path's 26,211 and 55,111
   (module bytes 21.34 MB, 20.34 MB, 19.72 MB); over the bench/study
   programs 2,509 and 3,324, then 1,949 and 3,019, against 818 and 1,430.
   Most of the rest are reads the old path borrows (commit 8). Move costs
   0.07 us per instruction with its rebuild, against insertion's 0.2, and
   allocates only per body (its rebuild's columns).
   **Left for later commits.** A part read of a base read again later
   copies, since liveness is per local and per temporary, not per field:
   in `.Square(n, k)` only `k` moves, and the copy of `n` and its drop
   are pair cancellation's (commit 9). The drops of moved locals after
   branches and loops stay until known-null (commit 10). Every read that
   could borrow still copies (commit 8). A generic base keeps its whole
   drop after a part moves out of it; splitting it would need `parts` to
   know which type parameters an erased instance leaves open, a fact no
   stage 2 commit plans. That drop releases only what the move left, so
   what it costs is the release glue's tests, not a count.
8. Borrow.
   As built: `own/borrow.rs`, between insertion and move. One walk in
   instruction order decides each `copy` read of a counted type. It
   borrows when every use lends (no consumer, no coercion, no `Ref` into
   the value itself) and nothing in its span writes what it reads, may
   reach it from outside the body, or suspends. The span is the
   statements of its `Drop`'s block from the one that holds the read to
   the drop. A statement's blocks are not always in order in the columns
   (a match's arms precede its decision tree, a guard's condition can
   precede the bindings before it), so the span is checked as the range
   of instructions from the first of any of its statements.
   - **Chains.** The read's place is resolved through the reads its root
     comes from (`%h = Read [$xs]` makes `[%h [%i]]` an element of `$xs`)
     and through binding locals, into a chain of roots, each with the
     place below it as step codes. Every write is indexed once under each
     root of its own chain, in rows sorted by instruction, and a span's
     check is a binary search per row. A write into a list's buffer or
     behind a reference keeps the header or the reference before that
     step, so it conflicts only with a read that goes past it; a write
     through the read's own value (an element store through a borrowed
     header) keeps that value. Each read in the chain also refuses what
     the verifier's overlap counts against its own place.
   - **Rule 3.** A chain ending at an unaddressed, unshared local is
     changed only by the body, calls included; so is its list's buffer
     when every value stored into the local is fresh (a call's result, a
     copy or snapshot, a literal), which shares its header with no other
     place. Anything else is the heap: no heap write and no call may
     stand in the span, but a call that may not write
     (`OwnCx::may_write`) for the one dereference of a parameter's
     reference read whole.
   - **Binding locals** (not in the plan): a local stored once with a
     read that borrows, no parameter, unaddressed, uncaptured, whose reads
     lie in the span (which then ends at the local's last `Drop`), holds
     a borrowed value and owns nothing: its drops go and it leaves the
     exit set. The loop's element binding is one, so with the element
     read of the private snapshot it is §23.25's loop-element loan, and
     `count_tests.hd`'s "immutable loop elements add no per-element
     counts" holds its 2,000-retain bound on the new path.
   - **Lent parameters** (not in the plan; `OwnCx::received`, a signature
     fact): a counted parameter of a function that does not suspend,
     which its caller lends (`OwnCx::pass`'s `Lent`) and the body never
     stores, addresses or captures, is the caller's for the call: its
     drops go, it leaves the exit set, and nothing in the body limits a
     borrow of it. The backend takes no count of it on entry and releases
     at the exit one an instance receives owned (a type parameter's).
     This is §23.31's parameter loans: without it a field read through
     `&self` copies, since the parameter's own drop follows its last
     read.
   - **Kept drops.** A borrow whose value an instance may make fresh
     keeps its drops: one through a dereference (a view's getter; the
     backend's flag, `Em::view_loans`, says whether the drop releases),
     and one naming a type parameter at its root or result (an erased
     body's thunk). Elsewhere the drop of a borrow releases nothing.
   - **Move** keeps every place a live borrow lives on live (a use of a
     borrow uses its root, and through a binding local what that holds),
     and moves no local outside the exit set.
   - **`verify_owned`**: the borrow clause is a backward liveness walk,
     one bit per borrow, loops to a fixed point, where a use of a value is
     a use of every borrow it lives on and a control instruction reads its
     operands before its blocks. A range of instructions, the old check,
     held a match's scrutinee live in its arms, where move may take the
     matched place. Binding locals and unowned parameters are checked to
     hold no count. Its use list is now `own::operands`; the old one read
     an `Await`'s provider words as values.
   - **The backend** lowers a `borrow` of every place form
     (`Em::prepare_own_loans`): it joins `Moves::loans`, and a store into
     a binding local counts nothing (`Moves::loan_locals`).
   - **Splicing**: borrow records its deletions in the edits move adds
     to, so the body is still rebuilt twice (insertion, then borrow and
     move), not three times.
   Old path: every conformance module byte-identical to main, dev and
   optimized pipelines. New path: all conformance cases pass, dev and
   optimized pipelines, checked and not, with no known failure left. Static count sites over
   the 769 runtime conformance programs with a `main`: 40,984 retains and
   79,072 releases before borrow, 27,489 and 59,420 after, against the old
   path's 26,839 and 56,174 (module bytes 20.52 MB, 19.97 MB, 19.88 MB);
   over the bench/study programs 2,140 and 3,304, then 955 and 1,883,
   against 918 and 1,579. Borrow costs 0.08 us per instruction (insertion
   0.22, move with the rebuild 0.11) and allocates only per body.
   **Left for later commits.** Releases stay above the old path's: a
   kept drop through a reference is a flag-tested release site, and the
   copies a later pass can cancel stay (commit 9). An element read of a
   list a parameter holds counts as the heap, since the caller's header
   may be shared, so a call in its span keeps the copy. A closure's
   parameters stay owned: `received` answers for a function's own
   signature only. Known-null (commit 10) must keep every owned
   parameter's exit row, since the backend reads a parameter outside the
   exit set as unowned.
9. Pair cancellation.
   As built: `own/pairs.rs`, after move, its edits joining borrow's and
   move's for the pipeline's second and last rebuild. It takes the case
   move leaves: a `copy` of a part `p` of a base move's candidates name
   (an owned local no reference, closure or cleanup suite reaches, or an
   owned temporary a `copy` reads a part of), whose base is read again
   later. It pairs with the first later instruction of its block that
   touches `p` or all of the base, when that is a `Drop` of `p` or of a
   place holding `p`, with only a straight line in between (no statement
   that runs a block, jumps, never completes or suspends), and nothing
   between touching `p`, all of the base, or a borrow that lives on it;
   for a temporary, nothing but other parts' moves and drops (the
   verifier's rule for a value taken in parts). The copy moves; a drop of
   `p` goes; a drop of a larger place becomes drops of the parts beside
   the path (move's `rest`, which now starts at the drop's place), or
   stays whole when a step is not laid out in place, releasing what the
   move left. So in `.Square(n, k)` the copy of `n` and the drop of `n`
   that the move of `k` left cancel
   (`own/a-part-copy-and-its-drop-cancel.hd`), and `[p.left, p.right]`
   takes both fields of a parameter with no count
   (`own/a-field-copy-and-its-drop-cancel.hd`). The plan's own example,
   a copy into a temporary dropped at the end of its statement while the
   place outlives it, is borrow's as built (commit 8): every such copy
   whose span holds no write of its place, no call that reaches it and no
   suspension point already borrows, which is this check and more; a
   count over the bench/study bodies found 29 copies dropped in a
   straight line with no call, write or uniqueness check between, and in
   each the drop of the copy's root stands between (a borrow's kept drop,
   below, a celled local's or a list header copy's), which no rule may
   cross.
   - **Data layout.** One backward walk over each block's statements and
     the edits pending in it (a deleted statement is gone, a new `Drop`
     stands at its position), keeping per candidate base (move's bits)
     the nearest later touch of all of it and of each first-level part:
     dense tables, stamped per straight line, no hash map. A touch counts
     at its first-level part, so a later touch of another part below the
     same first-level part ends a pair, as an overlap would. It costs
     0.025 us per instruction and 40 allocations over the bench/study
     programs (tables growing on the first bodies).
   - **`verify_owned`** tracks the parts taken of each value (a slot per
     distinct part place a move or a drop takes, a bit each on the walk):
     a value taken in parts stays owned until the parts taken on the path
     cover every counted leaf. It replaces "a part drop ends it", which
     reported a leak when two parts of one value moved.
   - **Not built: a `Ref` that counts a cell** (the plan's parenthesis).
     Cancelling it needs a `Ref` that takes no count: a new `ref_word`
     bit, its lowering (the cell's address, uncounted, except where an
     instance makes the reference a fresh view or a trait value, whose
     drop must then stay, as borrow's kept drops), and the verifier's
     borrow rule for references. The old path counts these references
     too, so they are no part of the gap; no stage 2 commit plans them,
     so they need their own issue after parity (commit 13).
   **Beyond the plan:** measuring the release gap found three places
   where the passes and the new path's emitter disagreed, each a count
   pair the passes could not see:
   - **List pushes and map stores consume their value** (`own::operands`:
     `ListPush`'s element, `MapSet`'s value). Insertion had them lend, so
     a pushed value took a retain inside the intrinsic and a `Drop` after
     it; the old path moves it in (§23.25, "a list or map store").
   - **An `@intrinsic` callee lends every argument** (`OwnCx::pass`). The
     backend lowers such a call in place and counts what it keeps
     (`Em::intrinsic`); a parameter declared a value made each argument a
     copy that the emitter released after the call (`bytes_at` in a loop:
     a retain and a release per byte).
   - **No release after an instruction that never completes**
     (`Em::release_untaken`). A `Return` or `Break` moves its value out
     and zeroes it; the release the emitter wrote after it was dead code,
     one release site per return of a counted value.
   Old path: every conformance module byte-identical to main, dev and
   optimized pipelines. New path: all conformance cases pass, dev and
   optimized pipelines, checked and not. Static count sites over the 769
   runtime conformance programs with a `main`: 27,489 retains and 59,420
   releases before this commit, 26,515 and 57,545 with the three fixes,
   26,447 and 57,477 with pair cancellation, against the old path's
   26,839 and 56,174 (module bytes 19.97 MB, 19.91 MB, 19.91 MB against
   19.88 MB); over the bench/study programs 955 and 1,883, then 822 and
   1,591, then 819 and 1,588, against 918 and 1,579. Rebased on main
   (771 programs): 28,111 and 60,510 before, 27,031 and 58,517 after,
   against 27,489 and 57,253 (20.13 MB, 20.07 MB, 20.05 MB). #722 holds on the
   new path: nbody's `advance` and a focused loop of f64 element-field
   stores through `&mut List` take no count. #681 does not: the callee's
   `Read copy [$0]` of `self` and the caller's `Drop` of the result are in
   two bodies, as [The known-bad cases](#the-known-bad-cases) says.
   **Left for later commits.** Release sites stay 2.3% above the old
   path's over conformance (0.6% over bench/study), most of it in
   suspending bodies: the new path's glue for a cancelled suspension point
   (`release_unwound`) releases everything the body holds at each `Await`
   (487 release sites at `Await`s in `task-with-timeout`, whose excess
   over the old path is 612), which commit 13's parity must share or
   remove. The rest are commit 7's whole drops of generic bases after a
   part moves, and commit 8's kept drops of borrows through a reference,
   whose drop ends before the copies read through them (`Read copy
   [%16 [%17]]` after `Drop [%16]`), so those copies cannot borrow.
10. Known-null.
   As built: `own/known.rs`, after pair cancellation, its deletions
   joining the other passes' edits for the pipeline's second and last
   rebuild. One forward walk per sub-body over the structured control
   flow, as the pending edits leave it (`splice::Pending`, which pair
   cancellation now shares: a deleted statement gone, a new `Drop` in
   its place), tracks one bit per owned local (an exit row's) and per
   owned value (and per `borrow` read with a kept drop): whether it may
   hold a count. A place is empty before its first store, after a
   `move` or a `Drop` of all of it, after the use that consumes it, and
   once the parts taken of it on the path cover every counted leaf (a
   bit per distinct part place, the verifier's rule over scratch stacks,
   `OwnCx::parts`); a store or a definition fills it. States join by
   union at branch and arm ends and at labels, and a loop's head runs to
   a fixed point; states live in one arena, last in, first out. A place a
   cleanup suite touches is never empty to the walk, since a suite runs
   at its scope's exits, which the walk does not follow.
   - **Rule 1**: a `Drop` (an instruction or a pending edit) whose root
     may hold a count on no path that reaches it goes: the drops after
     locals moved on every branch, every arm or every round (commit 7's
     leftover; `own/a-payload-moves-out-of-a-matched-temporary.hd`,
     `own/a-part-copy-and-its-drop-cancel.hd`).
   - **Rule 3**: a local empty at every way out of its sub-body leaves
     the exit set: each `Return`, the root's end, and each suspension
     point. A parameter's and a capture's rows stay (commit 8: the
     backend reads a parameter outside the exit set as unowned).
   - **The unwind set** (not in the plan): `Body::unwind_drops`, rows
     of a suspension point and a value that may hold a count there, a
     pass output the printer and parser carry and the wire refuses. The
     backend's `release_unwound` releases those values, for the point it
     cancels at or, from a cleanup scope's ladder, for every point inside
     the scope (`Em::cancel_from`), and no longer releases every value
     and local the body has: the cancel path leaves through the exit,
     whose exit set already holds every local that may hold a value at a
     suspension point (rule 3 counts the points as ways out). In
     `task-with-timeout`'s `main`, the unwind sets hold one to five
     values per point, and `main` has 342 release sites against the old
     path's 441 (671 before this commit).
   - **`verify_owned`**: at each suspension point, every owned value has
     an unwind row and every owned local that may hold a value has an
     exit row; the unwind rows are sorted, once each, and name a
     suspension point. Taking parts of a local now empties it once they
     cover every counted leaf, as for a value, so the exit-row check
     agrees with the pass (`std/json`'s `end_list` moves `closed._2` and
     drops the rest).
   It costs 0.06 us per instruction over the bench/study programs, and
   1,180 allocations over their 1,261 bodies (tables growing, once per
   run); the rebuilds stay at two.
   Old path: every conformance module byte-identical to main, dev and
   optimized pipelines. New path: all conformance cases pass, dev and
   optimized pipelines, checked and not. Static count sites over the 771
   runtime conformance programs with a `main` (on main at db25a6415):
   27,388 retains and 58,714 releases before this commit, 27,388 and
   53,398 after, against the old path's 27,914 and 57,467 (module bytes
   20.21 MB, 20.14 MB, against 20.19 MB); over the bench/study programs
   831 and 1,620 before and after (311,751 and 311,744 bytes), against
   935 and 1,599: outside suspending bodies the backend's Wasm zero facts
   (`opt::zero_releases`) already deleted the releases of these drops on
   both paths.
   **Not built.** The plan's rules 2, 4 and 5 change no count gap:
   - rule 2, a `Store` into a certainly empty place as an initialization,
     needs a mark the IR does not have (a `Store`'s two words are its
     place and its value), and outside suspending bodies the Wasm zero
     facts already delete the release of the old value on both paths;
   - rule 4, a `move` with no zeroing, also needs a mark (a read mode),
     and removes zero stores, not counts;
   - rule 5, cells made before use (#678), needs a "certainly full"
     state besides "may hold" and a fact per use; `Em::cell_made` reads
     `live::Tree` on both paths today. Commit 14, which deletes
     `live.rs`, must move it onto known-null's facts; rules 2 and 4 need
     their own issue after parity (commit 13).
   **Left for later commits.** Releases are now 7.1% below the old
   path's over conformance, but 151 programs keep 1,577 release sites
   above it (the others are 5,646 below), and bench/study 21:
   - **Drops before early returns.** A value held across `?`s is dropped
     before each `Return`, one release site each, where the old path
     releases once at its shared exit. Derived `Structure` walks
     (`HttpError.structure walk`: 20 drops of the matched value per
     instance against 2 releases) and `build`s read their by-value
     parameter whole into the match's scrutinee, which moves, since the
     parameter's own `Drop` follows the read and so borrow cannot keep
     it on the parameter. Sharing those releases at the exit is commit
     13's parity.
   - **Generic bodies** (`std/cmp`'s `EqualWalker.member`, `Iterator.sum`,
     `std/format`): commit 7's whole drops of generic bases and commit
     8's kept drops of borrows through a reference, and a type
     parameter's by-value argument, which `OwnCx::pass` lends, so the
     instance releases it at the exit where the old path moved it into
     the callee.
11. Hoisting and the stable-list table.
   **First, a fix to commit 7.** Move's rule 4 decided "not in a
   suspending body" by `Body::susp`, rows that only the IR text parser
   writes, so a `Ref` before an `Await` took its local's count of the
   cell (`own/a-suspension-point-unwinds-what-it-holds.hd`). A sub-body
   now suspends when it holds a suspension point, the instructions
   known-null gives unwind sets (`is_suspension`), and a celled local of
   such a sub-body (its exit row's) is no move candidate
   (`moves::suspending_subs`). `verify_owned` refuses a taking `Ref` in a
   sub-body with a suspension point. No other pass read `Body::susp`.
   **Hoisting as built** (`own/hoist.rs`), last in the pipeline, its
   edits joining the others' for the second and last rebuild
   (`splice::Pending`, as known-null reads it).
   - **Rule 1** is a statement of the loop's **entry test**'s iteration
     block: the loop body's first `If`, one arm of which only breaks out
     of the loop (`own::entry_test`; a `while` and a `for` over a range
     lower to it). The statements before the test must be pure (copies of
     values with no counted leaf, borrows and their kept drops,
     primitives, lengths, `and` and `or` of these), since the guard runs
     them once more before the loop.
   - **The place** is the one the `IsUnique`'s header read reads,
     resolved through value roots (`IsUnique [%h]` with
     `%h = Read [*%r]`, `%r = Read [$0]` is one of `[*$0]`): a local's,
     with one leading dereference at most and fields, whose type names no
     type parameter. Its local is never addressed or captured; no read of
     its header anywhere in the body has a use that keeps that very header
     (a store, an owned argument, a block's value, a coercion, a pin); and
     no `borrow` of it lives across the loop's entry, where the hoisted
     check will stand (`verify_owned`'s borrow clause).
   - **Rule 2** over the loop's instructions and the edits pending in
     its blocks (the range from the loop's first instruction to itself):
     no suspension point, `Pin` or `ListPin`; no `Ref` overlapping the
     place; no `Store`, `Drop` or `move` of the place or of one holding it
     (a write below it, into its buffer, is the point); no read of it whose
     value a use keeps or copies (`ValueCopy`, `SnapBegin`).
   - **Rule 3**: the private case is a whole local, not a parameter,
     whose every stored value is fresh (borrow's rule); for any other
     place no call may write (`OwnCx::may_write`), and nothing may write
     through another dereference or into another list's buffer, or
     change, copy or pin another collection, since a reference can reach
     the same list by another name (#710's rule).
   - **What moves.** Loops are taken outermost first. A place that passes
     gets one `IsUnique` before the loop (`unique_word::HOISTED` in its b
     word), and every `IsUnique` of it in the loop, at any depth, goes:
     the whole loop was checked, so an inner loop's or a branch's change
     needs no check of its own. One behind a dereference stays, marked
     `unique_word::COVERED`: in a module where a reference view is
     reachable a view's getter may make the header fresh on each read,
     and the backend runs it there; where no view is reachable it is
     skipped in the shape the link folds away.
   - **The stable-list table**, `Body::stable_lists`: rows of a loop and a
     place of the same form, read by an element read or store or a
     `ListLen` in the loop, that pass rules 2 and 3 with no `IsUnique` of
     them left in the loop and no push (a push may move the buffer). The
     `own` view prints them as `loop %n keeps [*$0]`.
   - **`verify_owned`** (`hoisted`): an `IsUnique` is plain, hoisted or
     covered; a hoisted one stands right before a loop (among other
     hoisted ones), names such a place of an unaddressed local, and in
     that loop nothing copies (a read whose value a use keeps), moves,
     writes or references it, pins or suspends, and no call may write
     unless it is a whole local no closure captures; a covered one names
     the place a hoisted one before an enclosing loop names; the stable
     rows name loops and such places, in order.
   - **The backend** (`hd_wasm_linear/DESIGN.md` §23.16): a hoisted
     `IsUnique` waits for its loop (`Em::hoisted`) and runs there under
     the entry test, the pure prefix emitted once more; unguarded in a
     suspending body or where a default argument's specialization folded
     the test away. One behind a dereference reads the reference as the
     referent's address, only where no view is reachable. The new path's
     loop caches are the table's rows (`Em::own_list_caches`), with no
     check before the loop and none of #710's analysis; the range proofs
     of element indices stay as they are on both paths.
   - **A fix to commit 8**: `OwnCx::may_write` never answered false for
     `f64.sqrt`, a body-less method of a float's inherent impl whose
     `@intrinsic` has no key; it now does, by the body table (never by
     decoding the body, which would run the passes on another body and
     deadlock on its cell). It changes no count over the conformance or
     bench/study programs; it lets nbody's `advance` hoist.
   Data layout: dense tables per instruction and local reused across
   bodies, every value's uses in compressed rows, the loops sorted
   outermost first, and per loop one scan for candidates and one to check
   them over the loop's instruction range (so an instruction is scanned
   once per loop around it). Hoisting costs 0.03 to 0.05 us per
   instruction over the bench/study programs (130 allocations over their
   1,263 bodies, tables growing), against insertion's 0.21 to 0.28 in the
   same runs on a loaded machine; the rebuilds stay at two.
   Old path: every conformance module byte-identical to main, dev and
   optimized pipelines. New path: all conformance cases pass, dev and
   optimized pipelines, checked and not.
   Static Wasm call sites over the bench/study programs, whole modules
   (std included), dev and optimized pipelines alike, old path, new path
   before this commit, after: `rt:list_unique` 171, 180, 170 (in `app`
   functions 25, 34, 25: nbody 5, 12, 5, fannkuch 8, 10, 8); `rt:map_unique`
   6, 6, 6; `rt:list_grow` 0, 134, 134 (the old path's push calls
   `rt:list_unique` itself, the new path's checks once and grows at the
   push). No program is above the old path's `rt:list_unique` count
   (json 134 against 135); over `bench/runtime/progs` and `bench/micro`
   17 and 16 (list-sort 3 and 2). A hoisted check is still one call site:
   what changes is how often it runs. (Commit 6's 19 against 45 counted
   on another basis that this commit could not reproduce.) Static count
   sites and module bytes, which hoisting does not change but for the
   guards and the caches: over the 771 runtime conformance programs with
   a `main` 27,450 retains and 53,460 releases before and after (module
   bytes 20,136,109 and 20,142,691), against the old path's 27,914 and
   57,467 (20,193,740); over the bench/study programs 831 and 1,620
   before and after (311,497 and 312,113 bytes), against 935 and 1,599
   (313,616). Against commit 10's note the new path has 62 more retains
   and releases over conformance: the references of the first fix, which
   count again in suspending bodies; the `sqrt` fix changes no count.
   Run time, V8, optimized pipeline, release profile, the median of nine
   alternating runs with process start (ms), old path, new path before,
   after: list-sort 794, 1,495, 764; micro sort 56, 56, 54; sieve 239,
   307, 233; fannkuch 95, 114, 92; nbody 118, 351, 235; spectral-norm
   181, 182, 186.
   **Left for later commits.**
   - **Generic places**: a place whose type names a type parameter does
     not hoist. An erased instance holds such a value as one counted
     view, and a local-rooted `IsUnique` would stay in the erased body,
     where the element layout is unknown; hoisting one needs hd_mono to
     outline it into a thunk as it outlines a value-rooted one. Commit 13
     counts what generic std loops lose and adds the outlining if parity
     needs it.
   - **Whole-element copies**: on the new path `bodies[i].x` reads the
     whole element (`Read copy [%h [%i]]`, then its field), where the old
     path loads one field (§23.31, "Read the selected field"); that, not
     hoisting, keeps nbody's `advance` slower on the new path. Commit 12
     roots element places at the local (`Read [*$0 [%i] .x]`), which reads
     one field.
12. The checker stops inserting copies and pin bookkeeping. Split in
    two: 12a gives the checker the switch and roots every place at its
    local; 12b removes the copies, the pins and `mark_write_path`.
    **12a as built.** The checker's TIR feeds both paths until commit 14,
    and the old path must stay byte-identical, so the checker takes the
    switch as one option, `BodyCx::own_passes`, which the driver fills
    from `Packages::own_passes` (the only reader of `HD_CONFORMANCE_OWN`
    stays the conformance test; the `own` view sets it, `hd` never). The
    driver's toolchain key names it (`a1=bounded;std=..;own`), so every
    cached artifact, the `check` entries holding the checker's TIR
    included, keys on it; the link key's own prefix went, the toolchain
    key covering it.
    - **Rooting** (`hd_tir::ir::root_places`, `src/ir/root.rs`). The
      checker still builds a place a step at a time, a read of the base
      and a projection rooted at its value, since its walkers read those
      values while it checks (`coll.rs`'s copies, `pins.rs`,
      `mark_write_path`, `Ck::element_of`). With the switch on,
      `finish_with` roots every such chain at its local before the
      verifier runs; off, nothing changes. A base read `%v = Read P`, `P`
      rooted at a local, folds into its uses when each is the root of a
      `Read`, `Store` or `Ref` place (a `Store` or a `Ref` going into
      memory, through a dereference or an element), the uses are
      statements of `%v`'s block, every statement between is inert (a
      read, a primitive, a construction, a coercion, a length; when `P`
      has an element step, which checks its index, only reads that check
      nothing, so no check moves before another), and the local's type
      names no type parameter or associated type. The chains fold from
      their root in one pass in instruction order, and the folded reads go
      in one rebuild (`own::splice`). So `bodies[i].x` is
      `Read [*$0 [%5] .x]`, `bodies[i].x = v` is `Store [*$0 [%5] .x] v`
      after an `IsUnique [*$0]`, `self.x` is `Read [*$0 .x]`, `grid[i][j]`
      is `Read [$0 [%1] [%2]]`. Bodies collection generates (supplied
      methods, `dbg` bodies) are rooted the same way before their passes
      (`Env::owned_generated`).
    - **Place folding** (commit 5's `own/fold.rs`) is gone: rooting folds
      every read it folded (a field of a whole local's read, the very next
      statement, a type naming no parameter) and more.
    - **What a place through the local touches.** A place rooted at a
      local lies in the local itself only when its steps are fields and
      variants (`Body::local_in_place`); a `Store` or a `Ref` through a
      dereference or an element writes or addresses a referent or a
      buffer. The passes and the verifier took a store or a reference
      rooted at a local as one into the local, which only such places had
      been: insertion's cells, borrow's stores, references and fresh
      values, move's addressed locals, known-null's refill, hoisting's
      addressed locals, and the verifier's addressed locals, cells,
      refills and unowned-local clause now ask `local_in_place`. Without
      it a lent `&mut` parameter written through (`bodies[i].x = v`)
      became owned again, one release more at the exit.
    - **Hoisting** takes an `IsUnique` of a place rooted at a local, and
      an element access's list from the place's steps before its first
      element (`hoist::rooted_place`), as it took them from the header
      read a value root named.
    - **The backend** (`hd_wasm_linear/DESIGN.md` §23.16, "Places rooted
      at the local"): one walk (`Em::walk_place`) resolves a place from
      its local's value step by step, a view's getter included, and a
      read loads only the leaves the place names; a store stores them; a
      reference is the address after the last element or dereference; an
      `IsUnique` or `Pin` takes the header the place names, through a
      dereference too. The loop cache and the range proof take the first
      element of a local's list or of the list its reference refers to
      (`Em::rooted_elem`), so the proven index of `bodies[i]` stays
      unchecked.
    - **The `own` view** lists a local a place reaches behind a
      dereference; `own/places-are-rooted-at-their-local.hd` shows the
      forms.
    Old path: every conformance module byte-identical to main, dev and
    optimized pipelines. New path: all conformance cases pass, dev and
    optimized pipelines, checked and not. Static count sites and module
    bytes, old path, new path before this commit, after: over the 771
    runtime conformance programs with a `main`, retains 27,913, 27,449,
    27,446, releases 57,465, 53,458, 53,201, bytes 20,193,656,
    20,142,627, 20,105,610; over the bench/study programs retains 935,
    831, 823, releases 1,599, 1,620, 1,601, bytes 313,616, 312,113,
    308,939: the base reads that went (a reference read before its
    dereference, a header before its element) took their own copies or
    kept drops. Run time, V8, optimized pipeline, release profile, median
    of nine alternating runs (ms; a loaded machine, two series): list-sort 782 and 766, 765 and
    746, 625 and 626; micro sort 77 and 57, 76 and 54, 74 and 55; sieve
    277 and 242, 293 and 238, 241 and 233; fannkuch 97 and 98, 94 and
    97, 95 and 97; nbody 120 and 122, 237 and 246, 98 and 101;
    spectral-norm 186 and 206, 188 and 191, 187 and 215. nbody's
    `advance` now loads one field per access (commit 11's "whole-element
    copies" leftover is gone) and beats the old path. The checker's cost:
    rooting takes 0.068 us per TIR instruction (2 us per body) over the
    bench/study builds, cold, std included (22,316 bodies), about 4.7% of
    the checker's `Body` stage there (1.46 us per instruction); the old
    mode runs none of it.
    **Left for later commits.**
    - **A base used as a value keeps its read**: a match's scrutinee,
      which its tag test reads, so its arms' payload reads stay
      `[%v as .V .k]` (a match on `xs[i]` reads the whole element); an
      intrinsic's collection operand (`ListLen`, `ListPush`, `MapSet`,
      `MapIndexMut`, `ListPin`), and the checker's `ListIndexMut`, so
      `xs[i].push(v)` reads the header before the element. The reads
      borrow, so they cost no count. `ListIndexMut` and `ListPin` go in
      12b; the intrinsics take values until stage 3 moves lists and maps
      to std.
    - **Generic locals**: a local whose type names a type parameter is not
      rooted, since an erased instance outlines an open read by its values
      (`hd_mono` would have to outline a place rooted at a local); commit
      13 counts what that costs the generic std loops, as it does for
      hoisting's generic places.
    - **A call between a base read and its use** (`xs[f(i)].x`) keeps the
      base read, and so a read rooted at its value: the element is read
      whole. Rooting a value-rooted chain needs the walk for value roots;
      commit 13 measures whether parity needs it.
    - The checker's copies (`ValueCopy`), `ListPin`/`ListUnpin` and
      `mark_write_path` stay in both modes (12b).
    **12b as built.** With the switch on, the checker leaves the pins and
    the element write marks to the passes; off, its TIR is the old path's,
    byte for byte.
    - **Pins.** An element reference is a `Ref` of the element place and
      nothing else (`refs::Root::Elem` holds no pinned local): no
      `ListPin`, `$pinned` local, `$pin` flag, `Defer` or `ListUnpin`, no
      call pins, and a suite that takes element references is no cleanup
      scope. Insertion's rule 6 pins the list. A `for` over element
      references reads its list once, as a value each step's `Ref` is
      rooted at; each step's reference gets an `IsUnique` and a `Pin`, and
      the step's binding, dropped after the step's `Match`, holds the pin
      to the end of the step. No code of the body runs between steps, so
      the list is pinned wherever the body runs (`flow.for.ref.access`;
      `own/a-loop-over-element-references-pins-each-step.hd`). The double
      `IsUnique` and `Pin` of commit 6 are gone.
    - **Write marks.** `mark_write_path` leaves an element read a read. It
      still turns a map value's read into `MapIndexMut`, an intrinsic that
      stays to stage 3: a `&mut self` call on a map value holds the value in
      a local first (`Ck::fit_receiver`), where no pass follows it.
      Insertion's rule 5 follows each change back through the reads it goes
      through (`insert::through`): from the collection operand of a push, a
      map store, a removal or a `MapIndexMut`, from an in-place argument,
      and from the value root of a `Store` or `Ref` place, through
      coercions and value-rooted reads up to a read of a local, a global or
      a dereference, it puts before each read an `IsUnique` (for a `Ref`,
      and a `Pin`) of every list on that read's path, outermost first, once
      per read. So `xs[i].push(v)` is `IsUnique [$xs]`, `%e = Read [$xs
      [%i]]`, `IsUnique [%e]`, the push.
    - **In-place arguments** (the owner's decision of 2026-10-22 on #728):
      `own::Pass::InPlace`, which `OwnCx::pass` answers from the declared
      parameter type (`Env::in_place`): the `mut` view of a list or a map,
      as a `mut self` receiver of one has; for a trait method's `mut Self`,
      when the call's receiver type is a list or a map. The argument is not
      consumed, as for `Lent` (the backend still counts it for a callee
      instance that owns it, `push_args`), and rule 5 puts an `IsUnique` of
      the places it reads before them and one of the argument before the
      call, so `grid[i].bump()` changes `grid[i]` with no copy
      (`own/a-mut-self-receiver-changes-its-place.hd`).
    - **Removed new-path code**: insertion's reading of `ListIndexMut`
      (`changed_elements`) and its `IsUnique` before `ListPin`; hoisting's
      and the verifier's `ListPin`/`ListUnpin` checks; the emitter's
      `ListPin` branch for owned bodies. Insertion refuses `ListIndexMut`,
      `ListPin` and `ListUnpin`, which only the old path's TIR has.
    - **Not done: the copies.** The checker still writes `ValueCopy` on
      both paths. It is no count: it makes a new list or map header over the
      same buffer, and that is what keeps a change through one holder from
      reaching another (`types.coll.value`). The passes' `copy` read is a
      count of the same header (commits 5 to 11 built it so, against the
      plan's "a list header copy in stages 1 and 2"), and two of their own
      rules need it to be: a change through a read value (`IsUnique [%e]`
      and the push above, an in-place argument) must reach the place the
      value was read from. With the checker's copies off on the new path,
      47 conformance cases and std's `list_tests.hd` fail ("the push changed
      ys only"). Moving the copies into the passes is a model choice for the
      owner: a `copy` whose value a consumer keeps makes a new header and
      a lent one shares it (the plan's model; a header allocation for each
      owned list argument still live after the call, where the checker
      shares), or `IsUnique` makes the header unique too and writes it back
      (stage 3's model early; it needs every change to read its place after
      the check, which the intrinsics' value operands do not), or insertion
      takes over the checker's rule (copy into a place that may change in
      place). Commit 13 does it once the owner picks; commit 14 then deletes
      the intrinsic as planned.
    Old path: every conformance module byte-identical to main, dev and
    optimized pipelines. New path: all conformance cases pass, dev and
    optimized pipelines, checked and not. Static count sites and module
    bytes, old path, new path before this commit, after: over the 771
    runtime conformance programs with a `main`, retains 27,913, 27,446,
    27,444, releases 57,465, 53,201, 53,198, bytes 20,193,656, 20,105,610,
    20,105,427; over the bench/study programs retains 935, 823, 823,
    releases 1,599, 1,601, 1,601, bytes 313,616, 308,939, 308,939. In the
    `app` functions of those conformance programs the passes' output held
    329 `IsUnique`s, one `Pin`, one `ListPin` and two `ListUnpin`s before,
    328, one, none and none after; bench/study takes no element reference
    and its `rt:list_unique` call sites stay at 170 (25 in `app`
    functions; 171 and 25 on the old path). Run time, V8, optimized
    pipeline, release profile, median of nine alternating runs (ms; a
    loaded machine, two series), old path, new before, new after: list-sort
    768 and 795, 619 and 646, 610 and 644; micro sort 58 and 58, 55 and 55,
    58 and 54; sieve 249 and 248, 247 and 248, 244 and 244; fannkuch 98 and
    100, 96 and 96, 96 and 96; nbody 124 and 120, 101 and 103, 101 and 100;
    spectral-norm 189 and 187, 188 and 192, 189 and 202. The checker's
    `Body` stage over the bench/study builds, cold, nine rounds: new mode
    939 ms before and 911 after, against the old mode's 899 and 871 in the
    same runs, so no change beyond noise: these programs take no element
    reference and change few elements.
    **Left for later commits (12b).**
    - **The copies** (`ValueCopy`, above): commit 13a, the owner's model
      3 (insertion takes over the checker's rule).
    - **A loop over element references checks per step**: its `IsUnique`
      and `Pin` run at each step, where the old path pinned once, since
      hoisting takes no loop that pins. Commit 13 measures it (no
      bench/study program has such a loop) and hoists the pair if parity
      needs it.
    - **A list holding references is pinned by a reference through it**:
      `&mut refs[i].x`, whose place lies in what `refs[i]` refers to, still
      gets an `IsUnique` and a `Pin` of `refs` (rule 6 counts every element
      step on the path), and so does the old path's `ListPin`; a push to
      `refs` panics `collection-pinned` on both paths while the reference
      lives. Whether such a reference pins is an open spec question
      (`ref.elem.place`), left for commit 13 to raise.
13. Parity: every counts baseline at or below today, conformance and
    the checked-mode audit green on the new path; the switch flips.
    Split in two: 13a moves the checker's collection copies into
    insertion (the owner's model 3 on #728); 13b is parity and the switch.
    **13a as built.** In new mode the checker writes no `ValueCopy`
    (`coll_copy`, `copy_into_local`, `mut_param_copies`, `value_arg`,
    `capture_copies` and `loop_binding_copy` return at once when
    `BodyCx::own_passes` is on); insertion writes the same intrinsic, by
    its new rule 7 (`own/copies.rs`). Off, the checker's TIR is the old
    path's, byte for byte.
    - **The checker's rule** (`coll.rs`, old path). A list or map value is
      a header over a buffer; a change in place makes the buffer the
      header's own and changes it through the header, so every holder of
      that header sees the change, and a holder of a copy (a new header
      over the same buffer) does not. The checker keeps a header that may
      change in place to one holder: a value whose type may hold a list or
      a map (`may_hold_coll`: a list, a map, a struct holding one in a
      field at any depth, a type parameter; a tuple, an enum or an
      optional shares, as no change reaches its members in place) that no
      instruction just made (`is_fresh`: a literal, a construction, a
      call's result, a copy) gets a `ValueCopy` when it goes into a place
      that may change in place, or comes out of one into a place that
      outlives the read:
      1. stored into a `let` or `:=` binding (`copy_into_local`), unless
         neither the binding nor the binding it is read from is `let mut`
         (a tuple element or a payload counts as such a read);
      2. returned (`return_copy`), unless it is the function's own `let
         mut` local, no parameter and captured by nothing, which the
         return moves out;
      3. stored through a reference (`*r = v`, `r.f = v`), held in a new
         object for `&v`, or the source of a struct update (`coll_copy`);
      4. a `mut` parameter, on entry (`mut_param_copies`);
      5. a value argument whose place something besides the caller may
         change while the callee runs (`value_arg`): a mutable global, a
         read through a reference, a `let mut` local a closure or a `&mut`
         reaches, or one a later argument changes; copied right after the
         argument, before its siblings; a list or map receiver passes as
         is;
      6. before a closure is made, a parameter or a binding that took one
         as is that it captures (`capture_copies`);
      7. a loop's element binding a closure captures (`loop_binding_copy`).
      A value stored into an element, a field, a payload or a tuple member
      is copied by the emitter's store glue on both paths (`slot_src`,
      `hd_wasm_linear/DESIGN.md` §23.17), not by the checker.
    - **Rule 7, as insertion states it.** The pass replaces the checker's
      syntax facts (`let mut`, `copy_locals`, the open loops and closures)
      by its own: a **changing** local is one whose header a change
      reaches in place, the root of a place rules 5 and 6 make unique or
      pin (through no dereference), of a value read from one, or of a
      `&mut` reference (to its storage, or into its list's buffer); it is
      **aliased** when that change can run while a callee runs: through
      such a reference, or inside a closure that captures it. A **fixed
      source** is a read of a place rooted at a local that is not changing,
      or a part of a fresh value. A value that may hold a list or a map
      (`OwnCx::header_copy`, the checker's `may_hold_coll` from the
      program's tables) and that is not fresh gets a `ValueCopy` for:
      1. a store into a whole local that is changing, or of a value whose
         source is not fixed; a map value read for a change
         (`MapIndexMut`), which the change must reach, never;
      2. the function's result (a `Return`, the function's root tail),
         unless it is a read of its own local (no parameter, captured by
         nothing) whose every stored value is fresh;
      3. a store through a dereference, and a struct update's source;
      4. a value argument whose source is a global, a dereference, an
         aliased local, or a local a later argument changes (a change site
         between the argument's read and the call), but an in-place
         argument (`Pass::InPlace`) and a list or map receiver
         (`OwnCx::receiver`);
      5. a changing parameter, on entry, but a `mut self` view of a
         collection (`OwnCx::in_place_view`): read, copied, stored back;
      6. a local a closure captures by value that may share its header (a
         parameter with no entry copy, or a local some value was stored
         into with no copy: a binding of a fixed source, a loop's element):
         read, copied, stored back before the closure is made.
    - **The same instruction, not a new statement.** The header copy is
      the `ValueCopy` intrinsic the checker wrote: a value that consumes
      nothing and makes a fresh header, which every later pass already
      reads as one (borrow's fresh stores, hoisting's copies, the
      verifier's balance), hd_mono outlines for an open value, and the
      backend lowers (`rt:list_copy`, a struct's copy of each list). A new
      ownership statement would be the same operation under another name,
      and stage 3, where a list's value is its buffer, turns it into a
      buffer retain either way.
    - **Placement.** A copy stands right before its use, after every
      other edit there (`splice::Tables::insert_late`), as the checker's
      did, and the value's own use ends at the copy, so its `Drop` stands
      between the copy and the use and a borrow of it spans nothing more;
      the use consumes the copy or lends it (then a `Drop` follows the
      use's statement). An argument's copy stands right after the
      argument, so a later argument's change cannot reach it, and a
      `Break`, `Continue`, `Return` or `ToArm` between drops it (a new
      jump item, `EDIT_BIT`). The entry and capture copies are `Read copy`,
      `ValueCopy`, `Drop`, `Store` back, the shape insertion made of the
      checker's.
    - **Sources.** A read's source is the local it is rooted at (through
      values it reads parts of), a global, a dereference, or a part of a
      fresh value; a map's `MapIndex`, `MapGet` and `MapSlotEntry` read
      what the map's table holds, so their source is the map's; a
      `MapIndexMut` is the map's value the change must reach.
    - **Where it decides less than the checker.** Three of the checker's
      copies are no copy under the pass's facts, each with its proof:
      - *a binding that is not changing, of a fixed source*: the checker
        copied into every `let mut` binding; the pass copies only into a
        local a change reaches in place, so a `let mut` that is only
        replaced whole shares a fixed source's header, and its result is
        copied instead where it leaves the function (2), as for any
        sharing local;
      - *a header that moves*: into a local that is not changing, a read
        of a local of the body's own at its last access, outside any loop
        and cleanup suite, that holds a header no other place holds (no
        parameter, captured, referenced or shared by a binding; every
        stored value fresh or copied) moves the header with no copy, and
        the new holder owns it alone (a comprehension's accumulator into
        its binding, which the checker read as a fixed local and did not
        copy either, but whose binding then copied at the result);
      - *a binding that shares safely*: a local stored once, captured and
        referenced by nothing, read only in its store's block after the
        store, by uses that are no call's argument (a callee may keep a
        reference to its parameter in its result, as `List.iter` does) and
        no block's value, shares its source's header when nothing that may
        change it in place stands between the store and the last use (for
        a local of the body no closure or reference reaches, a change of
        it; for anything else, a call or any change). A read of it stands
        for its source in later decisions (`link`). `SnapEnd`'s first
        operand reads nothing (the backend ignores it), so the source a
        `for` loop keeps in `$orig` shares the list, as the checker's
        compiler local did.
      And two it makes that the checker did not:
      - *a match binding of a source not fixed*: the checker copied only
        into the bindings a `let` names; insertion copies into any local
        it cannot prove safe (a binding read by a call's argument or
        outside its store's block, as a guard is). A binding of a map's
        list that a push then changes in place kept seeing the push on the
        old path (a defect of the old path,
        `own/a-match-binding-of-a-changing-map-value-copies.hd`, and the
        new path's `tests/all/header_copies_run.rs`; a conformance case
        lands with the switch, since the old path fails it);
      - *an argument of an aliased local through a desugared call* (an
        `Index` call `m[k]` on a local a `&mut` reaches): the checker's
        index desugaring skipped `value_arg`; insertion treats every call
        alike.
    - **`verify_owned`** (`header_copies`): a value that may hold a list or
      a map stored into a whole changing local is fresh (a copy included)
      or a map value read for a change, and a changing parameter, but a
      `mut self` view of a collection, is first read in its sub-body to be
      copied and stored back. The changing locals are recomputed from the
      output's own `IsUnique`s, `Pin`s and `&mut` references.
    - **`OwnCx`** gains three signature and layout facts: `header_copy`
      (whether a type may hold a list or a map, the checker's
      `may_hold_coll` from the program's tables, and the copy's type
      without its `mut` view), `in_place_view` and `receiver`.
    Old path: every conformance module byte-identical to main, dev and
    optimized pipelines. New path: all conformance cases pass, dev and
    optimized pipelines, checked and not. Static header copy sites (the
    `ValueCopy`s in the bodies the passes' output names), count sites and
    module bytes, old path, new path before this commit (main), after:
    over the 771 runtime conformance programs with a `main` (`app`
    bodies), copies 152, 152, 120 (results 64, 64, 46; stores into a
    binding 44, 44, 22; arguments 34, 34, 38; stores through a reference
    7; struct updates 3), retains 27,913, 27,444, 27,401, releases 57,465,
    53,198, 53,124, bytes 20,193,656, 20,105,427, 20,098,823; over the
    bench/study programs (every body), copies 63, 63, 57 (results 11, 11,
    8; stores into a binding 33, 33, 30; arguments 17; stores through a
    reference 2), retains 935, 823, 822, releases 1,599, 1,601, 1,595,
    bytes 313,616, 308,939, 308,572; its `rt:list_unique` call sites stay
    at 170 (25 in `app` functions; 171 and 25 on the old path). Five
    argument copies are new, one is gone. The new ones are desugared
    `Index` calls, which the checker's `value_arg` skipped: four `m[k]` on
    a local that a `&mut m` reaches elsewhere in the body
    (`a-tuple-key-stores-through-index-set`, after `fill(&mut m, 7)`
    returns; the whole-body aliasing below), and one `r[k]` through a
    `&mut` reference (`index-set-through-reference`). Run time, V8, optimized pipeline, release profile,
    median of nine alternating runs (ms, two series), old path, new
    before, new after: list-sort 763 and 765, 629 and 620, 618 and 619;
    micro sort 56 and 55, 54 and 53, 56 and 54; sieve 239 and 237, 234
    and 232, 234 and 232; fannkuch 97 and 96, 95 and 94, 93 and 94; nbody
    115 and 117, 100 and 100, 98 and 100; spectral-norm 185 and 182, 186
    and 185, 184 and 184; every output the same. The pass over the
    bench/study builds (1,263 bodies, 42,213 instructions, release
    profile, three runs): rule 7 takes 0.053 to 0.055 µs and 0.044
    allocations per instruction, a quarter of insertion's 0.22 µs (0.82
    allocations), about borrow's 0.062 µs.
    **Left for later commits (13a).**

    | Left | Why | Removed by |
    | --- | --- | --- |
    | The emitter's `slot_src` copies: a value stored into an element, a field, a payload or a construction's member (§23.17) | Both paths share the emitter's store glue; moving it changes the old path's modules | Commit 14, with the old path |
    | The match-binding conformance case (a match binding of a map's list sees a later `m["a"].push(3)`) | The old path fails it; until the switch, only the new path's `own/` fixture and `tests/all/header_copies_run.rs` hold it | Commit 13c, with the switch |
    | Whole-body aliasing: a local a `&mut` reference or a capturing closure reaches anywhere is aliased at every argument read | The checker's `copy_locals` is whole-body too; a flow-sensitive fact saves copies only before or after the reference lives | Kept: parity does not need it (13b) |
    **13b as built: parity.** Measured first on main (13a), new path
    against old: of the 49 counts baselines one stood above
    (`counts/match-field-through-reference`, a retain of the `Some`
    binding's reference); over the 773 runtime conformance programs with a
    `main`, 318 programs had more retain or release sites than the old
    path (1,249 retains and 1,475 releases above it, though the totals
    were below); over bench/study 4 of 12, over `bench/runtime/progs` and
    `bench/micro` 1 of 11. Each cause, and its fix:
    - **A type parameter's argument was lent** (`OwnCx::pass`): the
      passes could not move into `println[T](value: T)`, so an instance
      whose `T` is a value took a retain at the call and released the
      value after it (all 11 retains of `f32-arithmetic`'s `main`).
      A type parameter's value parameter is now `Pass::Owned`, as
      `OwnCx::received` already answered for the callee's side: an
      instance that makes it a value moves the argument in, and one that
      makes it a reference borrows it, after which the backend releases
      what the call consumed (`release_untaken`). That backend half was
      there; only the passes' answer changed. Where the instance borrows,
      `Em::instance_loans` takes back the retain and release of a
      parameter the body only lends or forwards to a direct callee that
      borrows it there too (`hash_of(a)` for `a: &string`).
    - **An owned parameter that only lends kept its own drop** (derived
      `Structure` walks and builds, `DebugShape.variant`): its `Drop`
      after its last read stood in every borrow's span from that read, so
      the match scrutinee read of it moved, and its value was then dropped
      before every `?` return. Borrow now leaves such a parameter unowned,
      as a lent one, when no read of it keeps its value and its body does
      not suspend (`OwnCx::suspending`, new): the instance releases its
      count once at the exit (§23.31's parameter loans), every read of it
      borrows, and the drops before the returns go with the scrutinee's
      copy (`own/an-owned-parameter-that-only-lends-is-the-callers.hd`).
      This, not sharing the drops at the exit, closed the early-return
      gap.
    - **Whole drops of generic bases after part moves**
      (`Result[_, P::Error]` after `?` in derived builds): in a concrete
      instance the whole release ran on a value whose one counted payload
      had moved. The backend's `Em::own_drop_rest` lowers such a drop, in
      an instance that lays the value out in its locals, as the release of
      the one counted field the moves before it in the block left, or
      nothing; an open value's parts were copies, so it releases whole
      there.
    - **Drops split per field.** Move's rule 3 split a base's drop into
      one drop per other counted field, a release site each where the old
      path's whole release is one call of the glue
      (`std/cli/Parsed.structure handle`: 7 sites for 2). The split is now
      made only when one counted field stays beside the path; otherwise
      the drop stays whole and releases what the move left
      (`own/a-moved-part-leaves-the-rest-to-one-drop.hd`). A part with no
      counted leaf no longer moves at all: moving a `bool` out of a
      `VariantInfo` split the drop into five for nothing.
    - **Drops after a statement ran oldest first**, so a match binding of
      a payload (younger than the scrutinee) had the scrutinee's drop in
      its span and could not borrow (`counts/match-field-through-reference`).
      Insertion now places the drops after one statement youngest first
      (`insert::after_drops`), and borrow's binding rule takes no binding
      that a read consumes, where the copy would only move to that read
      (`counts/match-owned-temporary` keeps its payload moves).
    - **References into a lent parameter's referent counted**
      (`self.source.next()` in `MapIter`, `Filter`, `Skip`, `Enumerate`:
      a retain and a release per call, dyn_iter's and closures' whole
      excess). Borrow marks such a `Ref` `lent` (`ref_word::LENT`, a new
      bit invariant 22 refuses before the passes and `verify_owned`
      checks): a field of what a parameter the body owns none of refers
      to, whose uses lend. The backend lowers it as §23.31's projection
      loan, uncounted where the referent is physical, its kept `Drop`
      releasing only what a view made
      (`own/a-reference-into-a-lent-parameter-takes-no-count.hd`).
    The parity causes the earlier notes expected but measurement did not
    confirm: **drops before early returns**, which the lent parameters
    removed, so no exit sharing of value drops was built; **whole-body
    aliasing** in rule 7 (the five extra `Index` argument copies), which
    puts no program above the old path; **generic places not hoisted or
    rooted** and **`xs[f(i)].x`**, which put no bench/study program above
    it in counts (`rt:list_unique` call sites 170 against 171, 25 in `app`
    functions on both paths; over `bench/runtime/progs` and `bench/micro`
    16 against 17) or in run time (below).
    **The two rebuilds stay.** Insertion rebuilds the body, and the five
    later passes record their edits for one more (`splice::Pending`, which
    pair cancellation, known-null and hoisting read). Folding insertion's
    rebuild into the last one would have borrow and move read insertion's
    new statements from side tables; both work on instruction ranges in
    column order (borrow's spans are `lo..=end` ranges searched by
    bisection, move's liveness walks blocks by their member lists), where
    a pending statement has no index. Giving them one is the rebuild. The
    rebuild costs about 0.015 µs per instruction, against the pipeline's
    0.5.
    #802 measured it again (release, bench/study, 42,188 instructions):
    the pipeline 0.48 to 0.50 µs per instruction, insertion's rebuild
    0.031, the last rebuild 0.018, and the three `splice::Pending`
    views (pair cancellation, known-null, hoisting; not rebuilds) 0.007
    together. The fold stays undone for the reason above: borrow and
    move address insertion's statements by column index in program
    order.
    Retain and release sites above the old path over conformance (summed
    over the programs above it), as the fixes landed: 1,249 and 1,475 on
    main; 66 and 600 with the type parameter's argument and the drop
    order; 65 and 46 with the lent parameters and the scalar part rule; 65
    and 16 with `own_drop_rest`; 49 and 8 with `instance_loans`; 49 and 1
    with the split rule; 44 and 1 with lent references; 45 and 1 with the
    binding rule, which keeps `counts/match-owned-temporary` at zero.
    Old path: every conformance module byte-identical to main, dev and
    optimized pipelines. New path: all conformance cases pass, dev and
    optimized pipelines, checked and not. Static count sites and module
    bytes, old path, new path before this commit (main), after:

    | Programs | Retains | Releases | Module bytes | Programs above the old path |
    | --- | --- | --- | --- | --- |
    | 773 runtime conformance programs | 28,805, 28,207, 25,764 | 59,935, 55,158, 50,277 | 20,499,058, 20,400,441, 20,265,273 | 318, 24 |
    | 12 bench/study programs | 935, 822, 761 | 1,599, 1,595, 1,372 | 313,616, 308,572, 302,814 | 4, 0 |
    | 11 `bench/runtime/progs` and `bench/micro` programs | 89, 97, 87 | 237, 225, 214 | 85,778, 85,012, 84,728 | 1, 0 |

    Over the 49 counts baselines, new path against old: 1 above, 31
    equal, 17 below on main; 0 above, 31 equal, 18 below after.
    **Left for later commits (13b).** 24 conformance programs keep 1 to 6
    retain sites above the old path (45 in all; one release), every one a
    case an instance decides that the passes cannot see:

    | Left | Why | Removed by |
    | --- | --- | --- |
    | A trait-value instance of a type parameter at a dynamic call (`notify[C](c: C)` at `C = &Channel`, `via`, `escalate`, `label`, `first_key`, `greet`, `disk_of`): the parameter, owned by the passes, takes a count on entry that its forward to the call releases | `Em::instance_loans` covers direct calls; a call through a vtable passes an open argument boxed, which a loan would have to keep uncounted inside the box | Not planned by any stage 2 or 3 commit; an issue after 13c |
    | Erased bodies and their thunks (`#erased`): an open value's `ValueCopy` or kept drop, +1 retain and release per body (`echo[T](x: T) -> T: x`) | An open value is one counted view; the old path moved it into the copy, which the passes' rule 7 copy, consuming nothing, cannot | Stage 3, where a list's value is its buffer and the header copy goes |
    | Payload copies of an unowned parameter or a borrowed scrutinee that a use consumes (`Option.map`'s `self`, `eval`'s `match e` through `&Expr`, `Expr.structure handle`) | A lent parameter's parts copy where an owned one's would move; a scrutinee read through a reference keeps its drop before each arm's jump, so its bindings cannot borrow from it | Not planned; the same issue (a binding that borrows across the arm needs the scrutinee's drop after the match) |

    **13c as built: the switch (#798).** The new path is the default.
    `Packages::old_ownership` replaces `Packages::own_passes` as the one
    opt-out (`HD_CONFORMANCE_OLD=1`), and the cache key carries `;old`
    for it. The conformance case
    `a-match-binding-of-a-map-list-keeps-its-value.hd` joins the suite,
    and `element-reference-copy-independent.hd` and its nested twin (#494)
    leave KNOWN_FAILURES.tsv; an old-path run lists the three as known.
    Three changes came with the switch:
    - A bound reference to a collection's `push` or a map's `remove` on a
      `let mut` local takes the local's address
      (`Ck::in_place_bound_receiver`). A shared header would get a list
      of its own from the passes' header copies, and the call would miss
      the local.
    - The passes renumber a body, but the line table resolves a call's
      anchor against the `check` entry's body. `Body::anchors` maps each
      instruction back to the checker's, so a panic trace keeps the
      frame of a call the passes moved (`std.testing.prop_with_case` in
      `cli/property-stateful-replay`).
    - The counts fixtures `f64-element-field-stores-*` and
      `counted-element-field-stores-keep-their-counts` hold #722 on the
      default path.

14. Delete the old path: `live.rs`, `count.rs`'s decisions, the
    loan and cache code, the checker's copies (`coll.rs`'s `coll_copy`,
    `return_copy`, `copy_into_local`, `mut_param_copies`, `value_arg`,
    `capture_copies`, `loop_binding_copy`), `ListPin`, `ListUnpin`; the
    `ValueCopy` intrinsic stays as insertion's header copy (13a) until
    stage 3 makes a list's value its buffer; update
    `hd_wasm_linear/DESIGN.md` §23.16, §23.20, §23.25, §23.31, §23.36.

    **14 as built, first part: `live.rs` and `count.rs`'s decisions
    (#799).** The emitter's moves, drops, loans of its own and their
    skips are gone; `count.rs` keeps the glue and the lowering of the
    passes' reads, drops and exit sets, and `Em::loans` and
    `Em::loan_locals` hold the borrows the passes write. The structural
    helpers `live.rs` also held (the block tree for the range proofs, the
    place shapes) moved to `emit/shape.rs`. Three things came with it:
    - **Known-null's rule 5 is built** (`own/known.rs`): an addressed
      local (a `Ref` of its own storage or a shared capture) gets a bit
      set where it may hold no value, cleared by a store into its own
      storage and set by a `Drop`, a `move` read and a taking `Ref`. A
      `Read`, `Store`, `Ref`, `IsUnique` or `Pin` rooted at the local, a
      `Closure` capturing it and a `Loop` get a row in `Body::full_cells`
      where the bit is clear on every path, and none inside a cleanup
      suite. `Em::cell_made` reads the rows instead of `live::Tree`'s
      dominance; the wat baselines that hold #678 are unchanged.
    - **A function reference's call body** (`call_func`) was the last
      body `live::plan` planned; it now runs through the passes like
      every other body (built by collection since #800, below).
    - **The old path stops at the backend.** `Input::owned` is gone: a
      body with an undecided `use` read is refused, so
      `Packages::old_ownership` (`HD_CONFORMANCE_OLD=1`) no longer builds
      a runnable program; #801 deletes it with the checker's old mode.

    **14 as built, second part: the loan and cache code (#800).** The
    emitter's own uniqueness decisions are gone: `Em::unique_explicit`
    (always on), the invariant-buffer check of a parameter's list
    (`invariant_unique`, `unique_once`, `cannot_share_buffers`), the
    backend's own loop projections and their uniqueness check before the
    loop (`begin_list_loop`'s candidates), `rt:list_unique`'s `push`
    mode, and the lowering of `ListIndexMut`, `ListPin` and `ListUnpin`,
    which insertion refuses. `Em::list_caches` holds only hoisting's
    stable lists (`Em::own_list_caches`); the range-index proof
    (`hd_wasm_linear/DESIGN.md` §23.37) stays. A function reference's
    call body is built by collection (`hd_mono::adapter_call_body`),
    which hands it to the driver for the passes
    (`ProgramEnv::own_generated`) and stores it as the adapter's instance
    body; the backend runs no pass.

    **14 as built, third part: the checker's old mode (#801).** The
    checker has one mode. `BodyCx::own_passes`, `Packages::old_ownership`,
    `HD_CONFORMANCE_OLD`, the toolchain key's `;old` suffix and the
    conformance runner's `old_path_known` are gone; `finish_with` always
    roots places, and the driver always runs the passes. Gone with them:
    - **The copies** (`coll.rs`): `coll_copy`, `return_copy`,
      `copy_into_local`, `mut_param_copies`, `value_arg` and its early
      reservations (`reserve_value_arg`, `value_receiver`,
      `settle_arg_copies`, the per-call `early` slots), `capture_copies`,
      `loop_binding_copy`, and the facts only they read (`copy_locals`,
      `alias_locals`, `arg_copies`, `unowned`, `may_hold_coll`).
    - **The pins** (`pins.rs`, now `elem_refs.rs`): pin frames, pin
      slots, `$pinned` and `$pin` locals, the unpin `Defer`s, the call
      receivers' pins and the binding owners of a pin; a suite with
      element references is no cleanup scope. `Root::Elem` holds the list
      itself, so an element reference is `Ref [%xs [%i]]` only.
    - **`mark_write_path`'s list rewrite.** An element read on a change's
      path stays a `Read`; a map value's read still becomes `MapIndexMut`.
    - **The intrinsics `ListIndexMut`, `ListPin` and `ListUnpin`**, and
      insertion's refusal of them. `IntrinsicOp` renumbers past them;
      every cached artifact keys on the compiler's build id, so no old
      entry reads under the new numbers.

    `ValueCopy` stays as insertion's header copy until stage 3. The
    checker's time over bench/study did not change measurably (cold, 5
    rounds, the `Body` stage's median: 829 and 867 ms before, 829 and
    853 ms after, in interleaved runs), since the new mode already
    returned from the copies and pins before they did any work.

    The passes now run with the backend's layout cache in place
    (`hd_wasm_linear::with_layout_cache`, a link's when they run inside
    one): `OwnCx::counted` walked each new type's layout from scratch
    outside a link, so a chain of 1,500 types held by value cost its cube
    (`recursive_layouts::a_large_recursion_group_and_long_chains_of_types`,
    251 s before, 9 s after).

### Map value places

Decision E's second job (#494): map values are places, as list elements
are.

- **The place.** `Proj::Elem` on a map is the value of an entry, at the
  entry's number, which `IntrinsicOp::MapEntry` (`(map, key) -> usize`)
  finds or panics `index-out-of-bounds` for. The checker's `Root::Elem`
  and `Root::Slot` hold the key, and `Ck::elem_index` emits the lookup
  just before each `Read`, `Store` or `Ref` of the place, after the
  stored value is evaluated, so nothing between can move the entry.
  `m[k].x += 1` looks the key up twice, as `xs[i].x += 1` checks its
  index twice. `Root::MapValue`, the fix-its that read a map value out
  and stored it back, and `map-value-not-addressable` on `m[k].x = v` and
  `&m[k]` are gone; a `for` over `&m` keeps that error
  (`flow.for.ref.map`).
- **Receivers.** `peek_place_ty` sees a map value, so a `&self` or
  `&mut self` call on `m[k]` refers to the value in the map, as on
  `xs[i]` (`fn.method.auto-address.collection-element`).
- **The passes are unchanged.** Insertion's rules 5 and 6 find the
  element projection whatever the collection: an `IsUnique` of the map
  before a store through its value, and an `IsUnique` and a `Pin` before
  a reference. `MapEntry` is inert for place folding and hoisting. Since
  `List`'s commit 2 a map value place folds to the local the map's read
  reads, `[$m [%e]]`, past that read's use by `MapEntry`, which keeps
  the read.
- **The backend.** A map's header is a list's kind, owner word included.
  `Em::map_value_at` addresses an entry's value; the element read, store
  and reference, and the rooted walk, take it for a map. `Pin` gives the
  header the slot of `rt:map_clone` (a new header over a whole copy of
  the table), so `ListCopy` of a pinned map copies its table, as for a
  list. `rt:map_unique` treats a header with an owner word as its
  table's one owner. A pinned map (`rt:list_pinned`, which reads the
  same two words) panics `collection-pinned` in `rt:map_put` on a new
  key and in `MapRemove` on a key it holds; replacing a held key's value
  keeps the entry and is allowed. Entry numbers survive `rt:map_unique`,
  which copies the table as it is, and only growth, which a pin forbids,
  renumbers entries.
- **`MapIndexMut` and the checker's mark are gone** (`List`'s commit 2).
  A plain read `m[k]` was `MapIndex`, which insertion's walk back through
  a change's reads did not follow, so the checker marked a read that a
  change went through `MapIndexMut`. Every read of a map value is now
  `Read [%m [%e]]`, so the walk follows it as it follows an element read.

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

- **Code.** About 1,300 lines of Rust code and 400 of tests, counted
  piece by piece in [the stage 3 size budget](#size-budget).
- **Compile time.** Only reached calls of small std bodies inline.
  Counted on json's 279 reached bodies (10,663 instructions after the
  passes), stage 3 adds about 4,900 instructions, +46%: about 2.4 ms of
  passes and 10 ms of emission. nbody adds about 290 (+11%); hello adds
  none while the inliner leaves `std.rt` alone
  ([compile time and code size](#compile-time-and-code-size)).
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
after the inliner lands. The owner approved the inliner on 2026-10-23,
after stage 2 measured the passes at 0.48 to 0.50 µs per instruction
over the 12 bench/study programs (#802): json's counted 4,900 inlined
instructions add about 2.4 ms of passes. With the inliner, `List`, its
cursor, `Map` and the string operations are all hd in std; the compiler
keeps the typed memory primitives and the inliner.

**Size budget first.** Stage 2's size table was a top-down guess: it
planned a net deletion of about 730 lines and landed about +12,600
(#49). Stage 3's [size budget](#size-budget) is bottom-up per
component, measured against the code each part replaces and against
reference implementations, and every stage 3 commit reports its lines
against its share.

- **Typed memory primitives** in a privileged std module: a pointer type
  over `T` with `size_of[T]`, `read`, `write` and `offset`, counted like
  `T`, next to today's untyped `std.rt.memory` loads and stores. This is
  a language-tier intrinsic family, approved by the owner on 2026-10-21;
  its signatures are plain hd declarations, and only their bodies are
  the compiler's.
- **Lists and maps as buffers, together.** A `List[T]` is the address
  of a counted buffer holding capacity, length, the owner word and the
  elements, and a `Map[K, V]` the address of its counted table, with the
  owner word at the same offset; the header object goes, saving an
  allocation per collection and a load per access. A copy of either is
  a retain of its buffer, or a clone when the owner word says the
  collection is pinned. An element place then needs one load less, and
  the buffer address is an ordinary value that hoisting can keep in a
  Wasm local. The owner decided on 2026-10-23 that lists and maps change
  together, so the compiler never carries two copy, pin or clone paths,
  nor a list-or-map choice per collection leaf
  ([List and Map buffers](#list-and-map-buffers)).
- **Option B for the list fast paths** (owner, 2026-10-23). `push`,
  `len` and indexing stay compiler-lowered over the buffer address; only
  the slow paths (growth, the copy that makes a shared buffer unique, and
  truncation) become std hd. If B proves insufficient, the first option
  to consider is a declaration-level `@inline` on a function or method,
  before widening the automatic inliner (inlining into copies, or
  reaching into `std.rt`). The other forty `List` methods are hd in
  `collections.hd` already.
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

### Size budget

Measured at 328bfa853; the `List` and `Map` row, the `for` row and the
intrinsics row measured again at ad3edc0f2, after `List`'s commit 2. A
"code" line is a Rust line that is not blank, not a `//` comment and not
inside a `#[cfg(test)]` module; an hd line is one that is not blank and
not a `#` comment, which is what the browser build embeds. Tests are
counted apart and never reach the web compiler.

| Component | Replaced (Rust code) | Added: Rust code / tests | Added: std hd | Reference point | Confidence |
| --- | ---: | --- | ---: | --- | --- |
| Inliner | 0 | 1,300 / 400 | 0 | Cranelift `inline.rs` 1,117; Go `inl.go` 848 + `interleaved.go` 291 + the inline-call part of `noder/reader.go` 157 | medium |
| Typed memory primitives | 0 | 510 / 200 | 70 | hd's own untyped `std.rt.memory` (19060a9b3): +250 / −58 Rust, 78 hd, 170 test lines | low |
| `List` and `Map` buffers, commits 3a to 3k | 2,690 | 885 (678 counted) / 450 | 235 | Go `runtime/slice.go` 232; Rust `vec/mod.rs` 1,330 + `raw_vec/mod.rs` 505; Go `internal/runtime/maps` 1,374; hashbrown `raw/mod.rs` 1,726 | medium for 3a to 3f, low for 3g (`copies.rs`) and the map commits |
| One `for` lowering | 960 | 180 / 120 | 20 | Go `walk/range.go` 346 | low |
| Strings in std | 320 | 30 / 40 | 80 | Go `runtime/string.go` 455 | medium |
| Deleting the intrinsics | 100 | 0 / 0 | −15 | none needed: pure deletion | high |
| **Total, as planned** | **4,070** | **2,905 / 1,210** | **390** | | |

The rows are plans; [stage 3's net](#stage-3s-net) puts the commits
already landed at their measured sizes and applies the calibration to
the rest: about −1,820 lines of Rust code and +350 of std hd. The
reference sizes are code lines measured the same way, from Go 1.24.1,
Rust 1.97 `library/alloc`, cranelift-codegen 0.136.2 and hashbrown
0.15.5.

**What each row replaces, and what stays.**

- **Inliner.** Replaces nothing. Its pieces: call-site selection and the
  size test (80); refusal rules for suspension, `defer`, closures and
  providers (50); body copy with local renumbering, constants and
  strings appended (150); label and sub-body offsets (90); a bulk splice
  of the copied instructions with every instruction operand renumbered
  (150); parameter binding by pass mode (60); `Return` to `Break` into
  a result `Block` (70); type substitution through the wire's id-word
  schema and `InternPool::subst` (120); re-choosing a substituted
  `Bound` call as an `Impl` (60); source positions for panics inside a
  copy (40); verifier invariants (40); driver hookup (70); the cache
  dependency list (40). That is 1,020; the 1,300 budget adds the
  stage 2 lesson below. It reuses `own::operands`, `splice`'s
  renumbering and `wire.rs`'s id words, so no per-tag remapper is
  written twice. **Done** in four commits: 1,002 code lines added and
  158 deleted (844 net) against the 1,300, of which about 250 are the
  inlined panic frames in the emitter and `hd_run`, and the verifier's
  copy checks are 70 rather than 40.
- **Typed memory primitives.** Replaces nothing; `std.rt.memory` stays.
  A `Ptr[T]`, `size_of`, `read`, `write`, `offset`, a counted buffer
  with capacity and length words, and two string views (its bytes, a new
  string of `n` bytes) as hd declarations (70 hd). Rust: `Ptr` as a
  known type and the std-only check in `hd_check` (60); a place rooted
  at a pointer's target, with its operand words and verifier rule, in
  `hd_tir` (120); reads and writes through a pointer in the ownership
  passes (120); lowering in `hd_wasm_linear` over the existing element
  slot code (150), and the buffer object kind with its element glue
  (60). **Done** in two commits: 337 Rust code lines added and 77
  deleted (260 net) against the 510, and 27 std hd lines against the 70.
  The checker's 60 were not needed, and the write-back rule took 126 of
  the passes' 120.
- **`List` and `Map` buffers.** One component since the owner's decision
  of 2026-10-23; [its own section](#list-and-map-buffers) counts it piece
  by piece and splits it into commits. It replaces what the separate
  `List` row (866) and `Map` row (2,234) planned to, less what option B
  keeps: the checker's built-in `len` and `push` and the lowered element
  places stay, and so do hoisting's stable lists, whose facts the buffer
  address still needs.
- **One `for` lowering.** Replaces the checker's `collection_loop`
  (122), `range_loop` (92), their dispatch (17) and most of `ref_loop`
  (about 100 of 128), the loop half of `emit/list.rs`
  (`begin_list_loop` to `own_list_caches` less hoisting's two entries,
  548) with `snap_begin` and `snap_end` (19), and the map loop glue the
  `Map` commits leave to it (`MapSlots`, `MapSlotNext` and
  `MapSlotEntry` with `rt:map_slot_next`, about 65): 960. Stays:
  `iterate`'s general `Iterable` path, which is the one lowering, and
  the element-reference conflict checks. Adds `for p in &mut xs` as a
  call (30), a `ListIterMut` (20 hd), and a fold of a `Match` on a
  variant the inlined `next` just built, so a range loop does not build
  an option per step (150).
- **Strings in std.** Replaces `str_eq`, `str_cmp`, `str_from_bytes`
  and `concat_all` in `rt.rs` (186), `str_at`, `str_slice`,
  `char_from_scalar` and `str_ordering` in `emit/text.rs` (90 of 226),
  their arms in `emit.rs` (16) and the checker's `+`, `==` and indexing
  on strings (25). Stays: string literals, host primitives, `format_int`,
  and the `Interp` kernel `concat` with its fixed-length fast path, so
  interpolation and `println` pull in no std `List` code. Adds 80 hd and
  30 Rust.
- **Deleting the intrinsics.** Under option B, `ListLen`, `ListPush`,
  `Item` and the checker's `Hit::Builtin` for a list stay, and the list
  and map `Helper` variants go with the `List` and `Map` commits. What is
  left: the string, snapshot and map-loop variants of `IntrinsicOp` and
  their table entries in `ir.rs` (about 25), their uses in `ir/root.rs`
  and `own.rs` (12), the keys in `hd_host_abi` (8), their dispatch arms
  in `emit.rs` (about 35) and the string `Helper` variants in `rt.rs`
  (about 20): about 100. The std stubs that name the deleted keys go too
  (15 hd).

**Calibration.** Stage 2's six passes average 860 code lines each
(`own/borrow.rs` 861, `insert.rs` 889, `moves.rs` 1,020, `pairs.rs` 394,
`known.rs` 1,057, `hoist.rs` 776, with `copies.rs` 894 and `splice.rs`
450 beside them), and its plan gave each about 200. The inliner's
bottom-up 1,020 is about one and a quarter passes; its closest pieces
here, `splice.rs` and the column handling of `wire.rs`, are about 850
together before selection, binding and substitution, so the budget is
1,300. For std hd, the one Rust-to-hd pair in the tree is the
allocator: `rt.rs`'s `alloc_class` and `free_class` (157 code lines
that write Wasm) do what 52 lines of `std.rt.alloc` do, about 3 to 1.
The list slow paths (`rt.rs`'s `list_grow`, `list_unique` and
`list_truncate`, 168 code lines) divided by 3 give about 55 hd lines, and
the `Map` commits' 170 hd lines are below `map.rs`'s 798 by 3 because
the key operations `emit/map.rs` generates per key type come from
`Hash` and `Eq` in hd.

**Stop rule.** Each commit's added code is its share: the inliner 1,300
over four, the primitives 510 over two, the `List` and `Map` buffer
commits as [their split](#list-and-map-buffers) gives them, the `for`
lowering 180 over two, strings 30 and 80 hd over two. A commit that adds
more than 1.5 times its share stops for review before it lands, and so
does a component's last commit when the lines it has deleted are under
two thirds of its replaced column.

**Biggest risk to the numbers.** It is now the `List` and `Map` buffers'
commit 3g: `copies.rs` and its verifier, 1,000 of the component's 2,690
deleted lines, go only if every count a copy of a collection takes runs
the copy glue ([its biggest remaining risk](#list-and-map-buffers)). The write-back that this paragraph
used to name as the risk is settled below: it is an ordinary store into
a named place. The second risk is in the `for` row: the 150-line fold is the
only thing between `RangeIter.next` and an option built per step in
nbody's six inner range loops, and `for p in &mut xs` as an iterator
of element references needs references returned from `next`, which the
element access model (#494) does not allow, so more of `ref_loop` may
stay.

**Settling the biggest risk** (2026-10-23, before `List`'s commit 1).
The TIR changes a list through its value: `xs.push(v)` was `%l = Read
[$xs]`, the arguments, `IsUnique [%l]` and `ListPush [%l v]`, and
`rows[i].push(v)` made `$rows` unique before `%l = Read [$rows [%i]]`.
That is sound only while `%l` is a header that every holder shares and
that never moves. With the buffer's address as the value, `IsUnique` may
replace the buffer (a shared one is copied), and so may growth, and the
new address must reach the place the list was read from.

- **The write-back is an ordinary store.** `IsUnique [p]` stores a
  copied buffer's address into `p`, which the emitter already walks for
  an element store; growth stores into the same place, from the
  emitter's `push`, or from inlined std code, where `Bind::Place` makes
  `self = grown` a `Store` into the argument's place (the inliner's
  commit 3).
- **It needs one pass rule: a change names its place and reads it after
  the check.** An `IsUnique` of a value has nowhere to store, and a value
  read before the arguments is the old address once the check copied the
  buffer. The check cannot move before the arguments either, since an
  argument may share the buffer (`xs.push(save(xs))`). So insertion's
  rule 5 makes the place unique, not the value, and when anything between
  the receiver's read and the change may write or call, the change takes
  a new `borrow` read of the place right after its `IsUnique`s. The
  receiver's own read stays first, used by nothing, since it evaluates
  the receiver and checks an element's index (`fn.method.eval-order`).
  That is `List`'s commit 1, about 80 lines; nothing of `copies.rs` is
  needed for it.
- **A `mut self` method's changes stay in its copy** (owner, 2026-10-23,
  replacing the copy-out decision of the same day). A `mut self` method on
  a list or a map gets its own copy of the receiver, exactly as on a
  struct, and the caller never sees its changes. In-place changes go only
  through `&mut self`, as std's `List.push` does. Nothing crosses a call
  by value, so no call copies a value out and no read needs a
  `conflicting-access` test.
- **Option B: `push`, `len` and indexing stay lowered** (owner,
  2026-10-23). The same copy-out would hold for std's `push`, and with
  today's inliner `push` would stay a call: its fast path is about 20
  instructions over `Buffer`'s accessors, which are calls into `std.rt`,
  which the inliner leaves alone, and which would stay calls inside the
  copy, since nothing inlines into a copy; the limit is 8. So `push`,
  `len` and indexing stay compiler-lowered over the buffer address, and
  only the slow paths (growth, the unique copy, truncation) become std
  hd. If that proves insufficient, the first option to consider is a
  declaration-level `@inline` on a function or method, before widening
  the automatic inliner to copy into its copies or to reach
  `std.rt.memory`.
- **`copies.rs` goes once lists and maps are both buffer addresses.**
  Its rules hold for every type `OwnCx::header_copy` names: lists, maps,
  and the structs and type parameters that may hold either. The owner
  decided on 2026-10-23 that both change together, so no commit stops
  `header_copy` for lists alone and keeps it for maps. A collection's copy
  is then a retain: every holder counts the buffer, a change's `IsUnique`
  copies a buffer two holders count, and a pinned collection's copy is
  made at once (`ref.elem.copy`) by the one copy glue, which reads the
  owner word from the buffer. `copies.rs` then goes but for about 10
  lines elsewhere: a parameter changed in place is owned, not lent
  (`borrow::lent_params` counts its `IsUnique` as the store that rule 5's
  entry copy is today), and the borrow pass already refuses a borrow
  across an `IsUnique` of its source's root. What that costs is the copy
  glue at every count a copy of a collection takes ([commit
  3g](#list-and-map-buffers)).
- **The header model's hole.** A list that a callee stores into a
  top-level binding keeps its caller's header, since no rule copies a
  store into a top-level binding, so after `n := save(xs)` and
  `xs.push(n)`, `saved` has the pushed element too (`types.coll.value`).
  With retains it cannot. Its runtime case,
  `runtime/valid/a-list-a-callee-stores-at-top-level-is-a-copy`, passes
  from commit 3f.

**The inliner's four commits**, each green on its own and reporting its
code lines against 325, the emitted sizes and the build time:

1. **Body copy**: `hd_tir::inline` copies a callee (locals, labels, constants, strings and records moved past the caller's; `splice`'s renumbering; `Store`s binding the parameters in the call's block; each `Return` a `Break` to the copy's root block, which an empty `Scope` owns; types by `map_body_types` and `hd_mono::subst`), and the driver copies item calls of small std functions outside `std.rt`, with unit tests.
2. **Trait calls**: a `ChoiceKind::Impl` call inlines with its impl's arguments matched from the receiver, and a `Bound` call in a copy that substitution makes concrete is chosen again as an `Impl`.
3. **Binding by pass mode**: a `mut` parameter, and an in-place collection receiver, write back through the argument's place, so those callees inline too, and a panic inside a copy names the callee's position.
4. **One splice per body**: all of a body's copies land in one rebuild, the verifier checks a copy's invariants, the cache dependency list records the callees, and the size limit is tuned again.

**As built.** Commit 1 copies item calls as planned; its empty `Scope`
is a plain block to the emitter, and the list-loop proof of an index in
bounds now accepts one too, so an inlined `len` in a loop body keeps the
loop's element reads unchecked. Commit 2 resolves a trait call whose
choice is an impl the way collection does: the impl's head is matched
against the receiver and the trait's arguments (`hd_mono::unify`), the
call runs the impl's method at those arguments and the method's own, or
the trait's default body at the receiver when the impl has no such
method. A trait value receiver and a derived newtype impl stay calls,
as collection dispatches them elsewhere. In each copy,
`inline::rechoose` turns a `Bound` call whose receiver and trait
arguments substitution made concrete into an `Impl` call of the impl
the solver selects; a compiler-supplied implementation leaves it a
`Bound`, which collection resolves the same way. A copy with a
parameter that may hold a list or a map waits for commit 3: a call
lends such an argument, but a `Store` copies its header, so an iterator
or a reference made in the copy would name the copy (`List.iter`'s
cursor would miss a later change to the list).

Commit 2 also brings forward commit 3's panic positions. A copy's own
instructions keep the callee's instruction as their anchor, with the
`INLINED` bit, and `Body::inlined` gives each copy's callee and the
call's anchor. The emitter numbers the inlined frames a function's
calls reach and writes them to an `hd.inlined` section (the callees'
paths, then per function each frame's path, instruction and parent
anchor), so the line table expands a row in a copy into the callee's
frame and then the call's, as if the call had not been inlined. `hd
build` resolves those frames into `hd.positions` and drops
`hd.inlined`. At limit 12, json inlines 113 call sites (21 of them
`Display.to_string`), nbody 2, and dyn_iter also the default bodies of
`Iterator.map`, `filter` and `sum`.

Commit 3 binds each parameter as the call passes it (`inline::Bind`,
which the driver picks from `OwnCx::pass` and `OwnCx::receiver`). A
parameter the call lends or gives is still a `Store` of the argument
into the parameter's local, but `Body::bound` lists that local, and the
header copies take the store as the call's argument: rule 4 instead of
rule 1. The local shares the argument's header unless the source may
change while the call runs (a global, a dereference, an aliased local,
or a local a later argument changes); then its copy stands right after
the argument, as an argument's does. A list or map receiver is never
copied, since a call passes it as is. A parameter the copy changes is
copied, as rule 5 copies it on entry, and a binding counts as a call's
argument in rule 1's test for a binding that shares its source, so
`view := values; view.iter()` still gives `view` a header of its own.
An in-place parameter (`Pass::InPlace`, the `mut` view of a list or a
map) has no local: the copy's places rooted at it are rooted at the
place its argument reads, so a change, or a whole store, writes back
through that place, and the passes see the change there. A struct's
`mut self` stays the callee's own copy, a `Store` like any other. The
collection-parameter refusal and the `mut`-parameter refusal are gone,
so `List.iter`'s cursor names the caller's list again. At limit 12, json
inlines 124 call sites: commit 2's 113, `Field.fact` 3, `Field.default`
2, `Field.has_default` 1, `JsonObject.len` 2, `List.is_empty` 1 and the
`Iterable.iter` of `JsonObject` and of `List`; closures adds
`List.iter`, and dyn_iter `List.iter`, `JsonObject.len` and
`JsonObject.iter`. No std function small enough takes an in-place
parameter today, so only the unit tests reach `Bind::Place`. Copying
`Field.fact` and its siblings into the serde walkers removes their
generic instances, so json's Wasm is 3.6% smaller than at commit 2.
Panic traces are unchanged; a stack-exhausting recursion through
`EqualWalker.member` now repeats 437 times instead of 481, since that
frame now holds the copied `Field.get` and `Variant.holds`.

Commit 4 puts all of a body's copies in with one rebuild
(`inline::inline_calls`). Each site is checked against the unchanged
caller; every caller instruction is renumbered once, past each copy at
or before it, so a call's number becomes its `Scope`'s; each copy is
renumbered straight to where it will stand and appended, in descending
call order as before; and one permutation puts the rows in place. A
rebuild per call was quadratic in a body's copies. The output is the
same: the 13 programs' Wasm is byte-identical to commit 3's, and a unit
test compares the two on bodies with two copies, where only the block
records a rebuild replaced keep different stale words. On json at
limit 12 the splices took 0.54 ms of the inliner's 2.25 ms and take
0.33 ms of 2.03 ms (medians of 20 cold builds); the rest is choosing,
decoding and substituting the callees.

`verify_owned` checks what a copy promises: each anchor with the
`INLINED` bit falls in a `Body::inlined` row, the rows are sorted and
disjoint and each call's anchor is the caller's own; a `Body::bound`
local is sorted, once, no parameter, and stored whole once, by its
binding; and a place in a copy is rooted at a local the body defines (a
parameter, or one a whole `Store` writes), so an in-place parameter's
local is never left in its copy. Checked-mode conformance passes with
them.

The cache dependency list is the owned body's hash, which folds in each
copied callee's TIR hash (commit 1). No cache entry below a program's
link reads it yet; the link key names every module's content, std's
through the toolchain key, so an edit to an inlined callee rebuilds its
callers. `inlined_callee_edit` builds std as the root package, edits a
copied callee's body and checks that the warm build emits what a cold
build of the edited sources does. A per-instance code cache must key an
instance by that hash, not by its own TIR's.

The limit, measured again with trait calls and binding by pass mode in,
as release `.wasm` bytes against 6792b1bfa, before the inliner:

| | before | 8 | 12 | 16 | 20 |
| --- | ---: | ---: | ---: | ---: | ---: |
| hello | 7,426 | 7,426 | 7,426 | 7,426 | 7,426 |
| json | 172,416 | 170,180 (−1.3%) | 171,956 (−0.3%) | 184,198 (+6.8%) | 185,955 (+7.9%) |
| map_count | 18,662 | 18,934 (+1.5%) | 19,000 (+1.8%) | 19,000 | 19,003 |
| string_build | 11,750 | 12,153 (+3.4%) | 12,174 (+3.6%) | 12,174 | 12,174 |
| binary_trees | 14,046 | 14,175 (+0.9%) | 14,175 | 14,175 | 14,175 |
| dyn_iter | 28,938 | 26,702 (−7.7%) | 26,702 | 26,702 | 26,702 |
| closures | 16,078 | 15,959 (−0.7%) | 15,959 | 15,959 | 15,959 |

The other six do not change between limits. 16 and 20 put json past
the +5% budget. 8 is the smallest everywhere and leaves hello alone, so
the limit is now 8. What 12 copies and 8 does not is mostly cold or
once-run code: `Parser.unexpected`, an error path, at 15 sites,
`TextReader.top` and `leave`, `JsonWriter.open`, `levels_of`,
`hex_symbol`, `Number.from_f64`, `StringBuilder.flush` and `build`. Run
times of json, dyn_iter, map_count and string_build stay within noise.
At 8, json copies 92 call sites (124 at 12), dyn_iter 68 (88),
map_count 8 (10), string_build 6 (8); binary_trees 7, closures 3,
fannkuch_redux 2 and nbody 2 do not change. The 192 runtime/panic
cases print the same bytes as at commit 3. `List`'s cursor and element
operations, at 30 to 40 instructions, need the limit tuned again when
they move to std.

**The typed primitives' two commits**, each reporting its code lines
against 255 Rust and 35 hd, the emitted sizes and the web compiler's gzip:

1. **The pointer and its operations**: `Ptr[T]`, `size_of`, `read`, `write` and `offset` in `std.rt.memory`, specified (spec 10 "Typed Memory") and lowered in the linear backend, with counting at the call: `read` moves the value's count out and `write` takes one for its copy, by the intrinsic call's convention (arguments lent, the result owned).
2. **The passes' view and what `List` needs first**: a place rooted at a pointer's target, with its operand words and verifier rule, so the passes move a last use into a `write`, cancel a read written back and borrow through a read; the counted buffer object kind with its capacity and length words and element drop glue; and the two string views.

The plan put all counting in commit 2. It moved into commit 1 because
the call convention already gives it in a few lines, and a commit 1
without it would have to refuse a counted `T` with a rule commit 2
retires, or count it wrong.

**As built.** Commit 1 adds 55 Rust code lines (`Em::ptr` 48, its call
arm 3, the four `hd_host_abi` keys 4) and 12 hd code lines, against 255
and 35. `Ptr[T]` is a plain struct of one `usize`, so it needs no known
type in the checker, and the existing `std-only-use` already keeps it
in std: the 60 lines planned for `hd_check` are not needed. `offset` is
an intrinsic rather than hd, since the inliner leaves `std.rt` callees
alone and an element access must not be a call. A `T` in the heap has a
list element's layout, so `size_of` is that layout's size. The 13
programs' Wasm is byte-identical, the web compiler's gzip grows 645 B
(1,897,145 to 1,897,790), and the std runtime tests gain
`ptr_tests.hd` (7 cases).

Commit 2 gives the passes a place for a pointer's target: root kind 4,
whose root word is the `Ptr[T]` value, written `[ptr %p]`. The checker
makes `p.read()` a `Read [ptr %p]` and `p.write(v)` a `Store [ptr %p]
%v` (`Ck::emit_call`), so neither is a call any more; `offset` and
`size_of` stay intrinsic calls. Invariant 20 lets a pointer root be only
the whole place of a `Read` or a `Store`, and 21 counts it writable. In
the ownership verifier a pointer's target is behind an address, so it
overlaps every place a reference can reach, and a read through it
empties nothing the verifier tracks: std's holding rule
(`module.rt.ptr.holds`) is what keeps a second read away. Insertion
reads through a pointer with `move`, which takes the heap's count, and
a `Store` consumes its value, so move's local rule 1 already moves a
last use into a write. The emitter (`Em::ptr_place`) loads the leaves
and counts by the read's mode, and stores with no release of the old
value. What the plan called cancelling a read written back and
borrowing through a read is one rule, `pairs::write_backs`, run between
move and pair cancellation: `x := p.read()` and a later `p.write(x)`
through a read of the same pointer local, in one block, with only
reads of other places, stores into unaddressed locals, `Prim`s,
constructions, drops of other values, the length and string intrinsics
and calls that may not write between them. The write and its read of
`x` go. The heap read copies when `x` is read again after the write and
holds no list or map header (`get`), and borrows when the write was `x`'s last access and every
other read of it borrows (`len`); `x` is then a binding local as the
borrow pass makes them: it holds a borrow and leaves the exit set.

The counted buffer is `std.rt.memory.Buffer[T]`, a struct of one private
address that the backend knows as `StdKind::Buffer` and lays out as one
counted address: the list buffer of `crates/hd_wasm_linear/DESIGN.md`
§23.7, made by `ListBuf` with the element glue `DropElems`, counted as
any counted address is, and traced by the collector as a list's buffer
is. So the 60 planned lines are plumbing: the kind, its known path, and
its rows in `layout.rs` and `cycle.rs`. `new` is an intrinsic
(`mem_buffer`); `capacity`, `len`, `set_len` and `at` are hd over the
untyped loads and stores and `Ptr`. A copy of a buffer is a retain.
`List` keeps its header until its own commits. The two string views are
`string_bytes(text) -> Ptr[u8]` (`mem_string_bytes`: the view's address)
and `new_string(n) -> string` (`mem_new_string`: a `Str` block, or the
static empty string for none).

Commit 2 adds 282 Rust code lines and deletes 77, against 455: the
pointer place, its text form and invariants 59 (`ir.rs` 41,
`ir/owned.rs` 10, `own.rs` 8), the passes 128 (`write_backs` 126,
insertion 2), the checker's lowering 12, the emitter 72 (`ptr_place`,
the buffer and string intrinsics, `layout.rs` and `cycle.rs`), and the
kind's plumbing 11 (`hd_mono`, `hd_resolve`, `hd_driver`, the three
`hd_host_abi` keys). The emitter's `read` and `write` arms go, and two
`layout.rs` matches shrink to one line each. std hd adds 15 code lines. The 13
programs' Wasm is byte-identical to commit 1's, and the web compiler's
gzip grows 2,925 B (1,897,790 to 1,900,715). Fixtures: three `own`
cases built inside std (the stage harness's new `std` build), and
`ptr_tests.hd` grows to 12 cases.

**`List`'s first two commits**, each green on its own, benchmarked (the
study runtimes of sieve, fannkuch_redux, map_count and json) and
reporting its lines against its share, as [settling the biggest
risk](#size-budget) orders them. They were planned as two of four; the
owner's decision to change lists and maps together replaced the last
two with [the `List` and `Map` buffer commits](#list-and-map-buffers),
3a to 3k.

1. **Changes name their place** (55 Rust): insertion's rule 5 makes the place a push's list or an in-place argument reads unique, not the value, and when anything between that read and the change may write or call, the change takes a new `borrow` read of the place right after its `IsUnique`s; headers unchanged. An operand rooted at a top-level binding, a map value (`MapIndexMut`) or another value keeps the check on the value until commits 2 and 3a.
2. **Map values as places for a change** (40 Rust): `m[k].push(x)` and an in-place call on `m[k]` read `[%m [%e]]` at `MapEntry`'s number, so a list in a map can take a new address; `MapIndexMut` and the map arm of the checker's `mark_write_path` go, the job [map value places](#map-value-places) left open. Headers unchanged.

**As built.** Commit 1 is `insert::changes` and `insert::own_place`.
Before the other insertion rules, `changes` finds each list change's
operand: a push's list and each in-place argument. One read of a place
rooted at a local, used by that change alone, names the place; each
other operand (a top-level binding's, a map value, a value-rooted
place) keeps the old check on its value. A named place's change counts
as an access of its local, so the local's `Drop` stands after the change
and not after the receiver's read. Where every instruction between the
read and the change is a read, an arithmetic step, a coercion or a
construction, the read is the operand and the `IsUnique`s of the lists
on its path and of the place itself stand before it. Otherwise the
receiver's read becomes a `borrow` that owns nothing and nothing uses,
and the change reads the place again right after the `IsUnique`s, which
stand just before it: `bags[i].items.push(twice(n))` checks `i` once
before the call and once at the push. Top-level bindings stay by value
because the emitter's place walk has none as a root yet; commit 3a adds
it.

Commit 1 adds 81 Rust code lines and deletes 23 (all in `insert.rs`),
against 55, under the stop line of 83. Four `own` baselines change as
the rule says, and an element push now reads its inner list with
`borrow` where it copied the header and dropped it
(`changes-make-the-collection-unique`); the new fixture
`a-change-reads-its-place-after-its-arguments` shows both forms. Release
`.wasm` against 3b2626f86: hello, fannkuch_redux, mandelbrot, nbody,
sieve, binary_trees and dyn_iter byte-identical; json 170,180 to 170,981
(+0.5%), string_build 12,153 to 12,253 (+0.8%), map_count 18,934 to
19,032 (+0.5%), spectral_norm +6 B, points −8 B, closures −4 B. Against
6792b1bfa, before the inliner, string_build is +4.3% and map_count
+2.0%, inside the +5% budget. The growth is the place walk that an
`IsUnique` of a place through a reference repeats where the header
model checked the value it had loaded: `self.parts.push(text)` in
`StringBuilder.push`, copied into string_build's and map_count's loops,
and json's `TextWriter`, whose `parts` is a `&mut List[string]` behind
`&mut self`. Commit 3d lowers an `IsUnique` and the read of the same
place right after it as one walk, which it needs anyway to store the new
address. The web compiler's gzip grows 1,028 B (1,900,715 to
1,901,743). Cold build CPU, medians of 11 interleaved release builds:
json 0.291 s to 0.292 s, nbody 0.250 s to 0.248 s. Study run times on
V8, medians of three interleaved rounds of 7 runs: sieve 234 to 239 ms
and fannkuch_redux 98 to 96 ms, whose Wasm is byte-identical, so the
noise is about ±2.5%; map_count 173 to 177 ms and json 120 to 122 ms.

Commit 2 makes every read of a map value a place. The checker's `m[k]`
is `MapEntry` and `Read [%m [%e]]`, as a store or a reference through it
already was, so `MapIndex` goes with `MapIndexMut`: insertion's walk
follows a map value's read as it follows an element read, and the
checker's `mark_write_path` with its three callers, the map arms of the
writable check and of the conflicting-access paths, `copies.rs`'s
`Src::Handle` and the verifier's exception for it go. Reading plain
values as places too was the simpler route: one form for every use of a
map value, and the intrinsic's emitter arm and pass entries go with it,
where keeping `MapIndex` would have needed the checker to rewrite a read
it had already emitted once it saw a change follow. Place folding then
roots a map value place at the local the map's read reads, past that
read's use by `MapEntry`, which keeps the read: `m["a"].push(4)` is
`%e = MapEntry [%m "a"]`, `IsUnique [$m]`, `IsUnique [$m [%e]]`,
`Read borrow [$m [%e]]` and the push, and the local's `Drop` follows the
push, which is commit 1's rule unchanged. A change whose arguments may
write keeps the check on the value its receiver read: an argument that
writes the map (`m["a"].push(grow(&mut m))`) may grow it, growth
renumbers entries, and a read of `[$m [%e]]` after it reached another
key's value when tried. Commit 3d must look the entry up again there
before a new buffer address can be stored back.

Commit 2 adds 46 Rust code lines and deletes 126, against 40, under the
stop line of 60. Two `own` baselines change from `MapIndexMut` to the
named place, and `a-map-value-is-a-place` roots its reference and store
at the local, its map read a `borrow` with no `Drop`; the new fixture
`a-change-through-a-map-value-names-its-place` shows both forms. The 13
release `.wasm` files are byte-identical to commit 1's (a program that
reads and pushes through `m[k]` in a loop goes from 11,610 to 11,603 B),
so the study run times move only by noise: map_count 178 to 178 ms and
json 120 to 122 ms, medians of three interleaved rounds of 7. Cold build
CPU, medians of 11 interleaved release builds: json 0.28 s to 0.28 s,
nbody 0.24 s to 0.24 s. The web compiler's gzip shrinks 239 B
(1,904,287 to 1,904,048, both built here). The header model's hole that
the settling above found has its runtime case,
`runtime/valid/a-list-a-callee-stores-at-top-level-is-a-copy`, a known
failure (#667) until commit 3f.

**Emitted size budget.** hello's Wasm does not change, and each
bench/study program's release `.wasm` stays within +5% of its size
before the inliner; a commit over that stops for review, like a commit
over its line share. Size is bounded by construction: the limit counts
a callee's instructions after substitution, only small callees inline
(12 instructions in commit 1, 8 since commit 4, well under the
estimate's 40, tuned by measuring), and nothing inlines into a copy within the same body. So
std's collection methods must keep their slow paths (growth, rehashing,
panic text) in separate functions too large to inline, and only the
fast path is copied. Commit 1 measured limits 8 to 40 on the 12
programs: at 12, json's Wasm is 1.8% smaller and nothing grows over 1%
(map_count +0.8%, string_build +0.9%); at 16 json grows 3.8%, at 40
5.8%.

Commit plan: the inliner (four commits, done), the typed primitives
(two, done), `List`'s first two (done), the `List` and `Map` buffers
(eleven, 3a to 3k, each benchmarked), the `for` lowering (two), strings
(two), deleting the intrinsics (one).

### List and Map buffers

Lists and maps change from a header to their buffer's address in one
series of commits (owner, 2026-10-23, replacing the order "`List`'s
buffer now, `Map` later"). Done in that order, the code would carry two
copy, pin and clone paths until `Map` landed, and every place that
copies each collection leaf of a value (`copy_elems`, `copy_src`,
`rt:map_unique`, `value_copy`, all of which call one `ListCopy` today,
since a map's header is a list's kind) would pick the list or the map
form per leaf. Together, both kinds keep the owner word at the same
offset of their buffer, so one copy glue reads it the same way for
either and, for a pinned collection, calls the clone glue the word
names, which is `rt:list_clone` or `rt:map_clone`. That is data, not a
per-leaf choice in the compiler.

Measured at ad3edc0f2, after `List`'s commit 2; lines are code lines as
[the size budget](#size-budget) counts them. "Counted" is the sum of the
pieces; a commit's share is that sum times 1.3, for the reason under
calibration below.

| Commit | Piece | Replaced | Added (counted) |
| --- | --- | ---: | ---: |
| 3a | insertion names a change's place rooted at a top-level binding (`root_local` takes a global root, with no local access to record) | | 12 |
| 3a | verifier: a global root for a change's operand | | 5 |
| 3a | emitter: `at_root` of a `PlaceRoot::Global` reads the binding's static slot or Wasm globals, and a store into the whole place writes them back; the two refusals go | 5 | 35 |
| 3a | `IsUnique` and `Pin` of a global place | | 8 |
| 3b | callee: each in-place parameter is one more result, pushed with no count by every `Return` and the root tail | | 40 |
| 3b | caller: the extra result is stored into the argument's place with no count change, by 3a's place store | | 20 |
| 3b | passes: the call is a write of that place for borrow, known-null and hoisting (insertion names it already) | | 15 |
| 3b | a trait-value slot for a `mut self` collection receiver, and a suspending callee's frame, carry the extra result | | 30 |
| 3c | the caller stores the marker 0 into the argument's place for the call when the place is reachable another way (a top-level binding, a cell, a dereference); the copied-out value replaces it | | 20 |
| 3c | a list or map read whose place starts at a top-level binding or passes a dereference tests for the marker and panics `conflicting-access` (one helper) | | 20 |
| 3c | the link folds the test away in a program that stores no marker, as it folds `RefViewsAbsent` | | 12 |
| 3d | map changes (`m[k] = v`, `m.remove(k)`) name their place in `insert::changes`, as a push does | 10 | 15 |
| 3d | a change through `[%m [%e]]` whose arguments may write looks the entry up again after its `IsUnique`s | | 20 |
| 3d | an `IsUnique` and the read of its place right after it lower as one walk | | 40 |
| 3d | verifier: a change's operand is a `Read` of its place right after that place's `IsUnique`s, with nothing between that may write | | 35 |
| 3e | layout: the owner word at payload offset 8 of a list buffer and of a map table; list elements from 16 (`LIST_ELEMS`), the map's `GROWN` at 16 and `INDEX` at 24, the static empty buffer 32 bytes, `CTX_DATA` and `CTX_VTABLE` after `LIST_ELEMS` | 15 | 10 |
| 3e | `rt:list_buf` and `rt:map_table` zero the owner word | | 4 |
| 3e | `list_copy`, `list_pinned`, `list_unique`, `map_unique`, `list_truncate` and `pin_owner` read and write it in the buffer | | 12 |
| 3e | growth and the unique copy carry it to the new buffer, since an exposed collection stays unshared after its direct pin ends | | 10 |
| 3f | header kernels: `rt:list_header`, `rt:list_header_as`, `ListHeaderDrop`, their variants, dispatch and checked kind | 49 | |
| 3f | `Em::header` and `Em::buffer` become one `Em::buffer`: the value, or a loop's cached buffer | 48 | 15 |
| 3f | `rt:list_copy` becomes the one copy glue `rt:coll_copy`: a retain, or a clone through the owner word's slot when pinned or exposed, the word given up otherwise; `ValueCopy` lowers to it | 47 | 30 |
| 3f | header loads and stores in `map.rs`, `emit/map.rs`, `emit/iter.rs`, `emit/refs.rs`, `boundary.rs`, `task.rs`, and the header calls of `NewList` and `NewMap` | 35 | |
| 3f | `rt:map_clone` over a table | 19 | 10 |
| 3f | `IsUnique` stores the buffer `rt:list_unique` or `rt:map_unique` returns into its place | | 20 |
| 3f | push's growth stores the new buffer into the operand's place; `list_truncate` writes through its reference | | 25 |
| 3f | `rt:map_put`, `rt:map_grow` and `rt:map_unique` take and return the table, and the caller stores it | | 25 |
| 3g | `own/copies.rs` | 887 | |
| 3g | the verifier's `header_copies` (invariant 23) | 112 | |
| 3g | `OwnCx::header_copy`, `in_place_view` and `HeaderCopy`, the test context's, and the driver's with `holds_coll` | 62 | |
| 3g | insertion's call and `ValueCopy` site, the `ValueCopy` cases in pairs, hoisting and borrow | 21 | |
| 3g | the emitter's `value_copy` and the slot copy of `copy_src`, `slot_src` and `fresh` | 61 | |
| 3g | count glue: a retain of a value's collection leaves runs the copy glue and replaces the leaf (`count_locals`, the `Retain` glue) | | 30 |
| 3g | heap copies: `RetainFields`, and a value stored into an element, field or payload slot | | 25 |
| 3g | task frames and boundary arguments that take counts (`own_arg`) | | 10 |
| 3g | a parameter changed in place is owned (`borrow::lent_params`) | | 10 |
| 3g | verifier and fixture plumbing | | 15 |
| 3h | `rt:list_grow`, `rt:list_unique`, `rt:list_truncate`, their variants and dispatch | 176 | |
| 3h | the lowered push, `IsUnique` and `list_truncate` call std's slow paths by name with the element's glue; the driver roots them | | 30 |
| 3j | `map.rs`'s probe, put, growth, index and hash glue with their address helpers | 482 | |
| 3j | `emit/map.rs`'s `map_op`, `map_put`, `map_len`, `table_field` and the per-key operations (`key_ops`, `key_own`, `key_params`, `map_key`) | 212 | |
| 3j | the checker's map built-ins, `hd_mono`'s map key choice and the driver's key items, the map `Helper` variants and dispatch | 125 | |
| 3j | the checker resolves `get`, `set`, `remove`, `len` and `m[k] = v` as std calls; `MapEntry` is a call of std's entry number; emitter rest | | 40 |
| 3k | `rt:map_unique`, `rt:map_clone`, `rt:map_step`, `rt:map_table`, `Em::new_map` and `emit/iter.rs` | 320 | |
| 3k | `NewMap` as calls of std's construction; a map's `IsUnique` and its owner word's clone slot name std functions | | 30 |
| | **Total** | **2,686** | **678** |

std hd adds 235 lines: in 3e one changed line (`Buffer[T].at` at offset
16); in 3h growth (20), the unique copy (15) and truncation (12), about
50; in 3i the table type over its buffer (15), hashing with the seed (8),
probing (25), insert (30), growth (25) and remove (15), about 120; in 3j
`get`, `set`, `len`, `contains`, indexing (20) and a value place's entry
number (10); in 3k `MapEntries` (15), the literal's construction (10),
and copy on write and clone (10). Tests add about 450 lines: `own`
fixtures per commit, 3c's conformance cases, and std tests for 3h, 3i
and 3j.

**What stays.** Element places with their bounds check, insertion's
`IsUnique` and `Pin` rules 5 and 6, `NewList`, `ListPush` and `ListLen`
(option B), the checker's built-in `len` and `push`, hoisting's stable
lists (the facts hold for a buffer address), `rt:list_buf`,
`rt:list_clone`, `rt:copy_elems` and the buffer's drop and cycle glue,
context buffers, the entries' drop glue, `map_value_at` with the entry
layout a map value place needs, the hash seed intrinsic, and the three
map-loop intrinsics with `rt:map_slot_next`, which are the `for` row's.
3i keeps today's table layout (counts, owner word, index, hashes and
entries in one block), so the Rust glue and the hd code agree while
both exist.

**Reference points.** A list buffer that holds its own capacity and
length behind one counted address, made unique before a change, is
Swift's `Array` (`ContiguousArrayBuffer` with `isUniquelyReferenced`);
Go and Rust keep length and capacity in the value instead. The growth,
unique copy and truncation that move to hd do what Go's
`runtime/slice.go` (232) and Rust's `raw_vec/mod.rs` (505) do, and hd's
50 lines over `Buffer[T]` are smaller because there is one growth policy
and no allocator parameter. The map table in hd does what Go's
`internal/runtime/maps` (1,374) and hashbrown's `raw/mod.rs` (1,726) do,
with a dense insertion-ordered table instead of a Swiss table. The
`conflicting-access` check is a dynamic exclusivity check in the sense
of Swift's SE-0176 (dynamic enforcement for class properties, globals
and escaping closures); hd's is narrower, since it covers only a
collection that a non-inlined call changes in place. No Swift sources
were measured here.

**Calibration.** Stage 2's top-down table planned −730 and landed about
+12,600. Stage 3's bottom-up shares so far, measured with this budget's
counter per commit: the inliner added 1,002 against 1,300 (its
bottom-up 1,020), 844 net; the typed primitives 337 against 510 (the
checker's 60 were not needed); `List`'s commit 1 added 81 against 55
(1.47 times), commit 2 46 against 40 (1.15 times). New code in a module
of its own met or beat its count; changes threaded through existing
passes and emitter code ran 1.15 to 1.5 times it, and the old commit 3's
share of 110, set top-down, was a fifth of its 500 to 650 (c8ea10160).
Every piece above is threaded code, so each commit's share is its count
times 1.3, and its stop line is 1.5 times the share.

| Commit | Rust share (stop) | std hd (stop) | Deletes | Emitted size against 6792b1bfa | Web gzip | Judged by |
| --- | ---: | ---: | ---: | --- | --- | --- |
| 3a places at top-level bindings | 80 (120) | 0 | 5 | the 13 programs byte-identical: none changes a list through a top-level binding | +0.9 KB | sieve and fannkuch_redux as noise, json, map_count |
| 3b copy-out | 135 (205) | 0 | 0 | byte-identical: std and the 12 programs have no in-place parameter | +1.7 KB | as 3a |
| 3c the `conflicting-access` check | 70 (105) | 0 | 0 | byte-identical as shipped; json and string_build also built with the fold off, to report the check's size and time | +0.9 KB | json and string_build, fold on and off |
| 3d every change names its place, read in one walk | 145 (220) | 0 | 10 | string_build back to about +3.5% from +4.3%, json at most −0.8%, map_count at most +2.0%, the rest byte-identical | +1.7 KB | string_build, map_count, json |
| 3e the owner word in the buffer | 45 (70) | 1 | 15 | within ±0.3% of 3d: one more load in the pin and owner tests of the runtime glue | +0.4 KB | sieve, fannkuch_redux, json, map_count |
| 3f the buffer address | 165 (250) | 0 | 198 | smaller: a load less per element access, length and push, no header allocation; json, string_build, map_count, sieve, fannkuch_redux and nbody about −0.5% to −2%; hello unchanged (it makes no list) | −0.4 KB | sieve, fannkuch_redux, nbody, spectral_norm, json, map_count, string_build; json and nbody build CPU |
| 3g no header copies | 115 (175) | 0 | 1,143 | within ±1%: json's 22 `ValueCopy` calls go, and each count of a collection leaf calls the copy glue and stores its result | −12.9 KB | json, closures, dyn_iter, map_count |
| 3h list slow paths in std hd | 40 (60) | 50 (75) | 176 | string_build sets the limit: about 2% to 3.5% of room (230 to 410 B) after 3d and 3f, and std's instances for `List[string]` may take 200 to 600 B | −1.3 KB | string_build, sieve, fannkuch_redux, json |
| 3i the map table in std hd, not yet called | 0 | 120 (180) | 0 | byte-identical: no program instantiates it | +1.0 KB | std tests only |
| 3j map operations as std calls | 50 (75) | 30 (45) | 819 | map_count sets the limit: +2.0% today, about +1% after 3f, and std's probe and insert instances against `rt:map_find` and `rt:map_put`: 0 to +3% | −9.4 KB | map_count, json |
| 3k map literal, copy and iteration in std hd | 40 (60) | 35 (55) | 320 | within ±1% of 3j | −3.2 KB | map_count, json |
| **Total** | **885** | **235** | **2,686** | hello unchanged throughout | **−20.6 KB** | |

hello's Wasm does not change in any of them, and each of the 12
bench/study programs stays within +5% of its size at 6792b1bfa; a commit
over that stops, like one over its stop line. The gzip column uses 12.5
B per Rust code line (the measured 11 to 14) and 8.2 B per hd line, from
the web compiler's 1,904,048 B after `List`'s commit 2: the largest
point is about 1,910 KB after 3e, the end about 1,883 KB, far under the 2
MB cap. Study times are medians of three interleaved rounds of 7 runs,
whose noise was about ±2.5% at `List`'s commit 1.

**The commits**, each green on its own and about an hour of agent work:

- **3a. Places at top-level bindings.** A change's operand read from a
  place rooted at a top-level binding names that place, as a local's
  does since commit 1, and the emitter walks and stores a global root.
  Headers unchanged. It depends on nothing about maps, so it may land
  first.
- **3b. Copy-out.** A non-inlined in-place call returns each in-place
  parameter's final value, and the caller stores it into the argument's
  place. Under headers the returned value is the same header, so nothing
  observable changes yet; the commit exists so that 3f's buffer address,
  which growth or a unique copy may replace inside the callee, always
  reaches the caller's place.
- **3c. The `conflicting-access` check.** The marker, the read test and
  its fold, the spec rule next to `ref.elem.access.pin`, `types.coll.nested`
  reconciled, and conformance cases: a user trait's `mut self` method on
  a list whose callback reads the list through a top-level binding. The
  marker is 0, which no list or map value is, since an empty one is the
  static empty buffer. The check stays on in dev and in release, as the
  owner decided; the fold only removes it from a program in which no
  call stores a marker, where no read can see one, so json and
  string_build pay nothing as shipped and the commit reports what they
  would pay with the fold off.
- **3d. Every change names its place, read in one walk.** Map changes
  name their place, a change through a map value whose arguments may
  write looks its entry up again (growth renumbers entries), an
  `IsUnique` and its place's read lower as one walk, and the verifier
  checks the form. All of it holds under headers, it is what 3f's
  write-back needs, and the one walk wins back what commit 1 cost
  string_build.
- **3e. The owner word in the buffer.** For lists and maps at once.
  Headers stay and still hold only the buffer's address; the one copy
  path is still `ListCopy`. A buffer whose owner word is set still has
  one header holding it, since a copy gives the word up or clones, so
  the invariant is today's. This isolates the layout change (contexts,
  `Buffer[T]`, the boundary, the collector) from the value change.
- **3f. The buffer address.** For lists and maps at once: the value is
  the buffer's address, every change that may replace it stores the new
  address into the place it names, and `ValueCopy` is the one copy glue.
  `header_copy` still answers, which is sound with retains: `copies.rs`
  lets two holders share only where nothing changes or pins the
  collection while both live. The known failure
  `runtime/valid/a-list-a-callee-stores-at-top-level-is-a-copy` passes,
  and its `KNOWN_FAILURES.tsv` row goes.
- **3g. No header copies.** `header_copy` goes for both kinds, and with
  it `copies.rs` and its verifier. Every count a copy of a collection
  takes runs the copy glue, so a pinned collection's copy is a clone
  (`ref.elem.copy`), and a parameter changed in place is owned.
- **3h. The list slow paths in std hd.** Growth, the unique copy and
  truncation in `std.rt` over `Buffer[T]`, replacing `rt.rs`'s kernels
  once measured at least as fast; until then the kernels stay. Emitted
  code calls std functions only by name (`RUNTIME_FNS`); growth moves
  bytes and needs no element type, but the unique copy and truncation
  count elements, so they take the element's glue or need an instance
  per element type, which is this commit's risk.
- **3i. The map table in std hd.** Lookup, insert, growth and remove
  over today's layout, with std tests, called by nothing yet.
- **3j. Map operations as std calls.** The checker lowers `get`, `set`,
  `remove`, `len` and `m[k] = v` as calls of 3i's methods, so the
  monomorphizer collects their instances, and the Rust probe and insert
  glue, the per-key operations and `hd_mono`'s map key choice go; kept
  only if `map_count` is at least as fast.
- **3k. Map literal, copy and iteration in std hd.** `NewMap`, the
  unique copy and clone a map's `IsUnique` and owner word name, and
  `MapEntries`' step; `emit/iter.rs` goes.

**As built.** 3a roots a top-level binding's places as a local's and
lets a change name them. Place folding (`ir::root_places`) folds a
chain that starts at a binding as one that starts at a local, so
`bags[i].items` is `[@bags [%i] .items]` and `config.names` is
`[@config .names]`; the checker's reads of a binding were all values
before, so the plan's insertion piece alone would have named only
`saved.push(n)`. A binding's type is never open, so folding's third
rule does not apply. Insertion's `changes` takes an operand read from
such a place as rooted, with no local access to record:
`saved.push(twice(n))` is `Read borrow [@saved]`, which only evaluates
the receiver, the call, `IsUnique [@saved]`, a new `Read borrow
[@saved]` and the push, as for a local. The emitter's walk starts at the
binding's static slot, or at its Wasm globals read into locals
(`Em::at_root`). A place rooted at a binding takes the walk when it has
a step other than a field, or when it is read through fields; a store
into or a reference to a binding's fields keeps the old lowering
(`global_field_store`, the static slot's address), which already writes
it. `IsUnique` and `Pin` of a binding's place need nothing more, since
`place_header` walks. No store writes a whole binding through the walk
yet: under headers an `IsUnique` replaces nothing, so the write-back the
plan priced here comes with 3f's stores of a new buffer. The verifier
needed no change; checked-mode conformance passes.

3a adds 46 Rust code lines and deletes 15 (`ir/root.rs` +13/−9,
`own/insert.rs` +6/−2, `emit/refs.rs` +27/−4), against 80, under the
stop line of 120. No baseline changes; the new fixture
`a-change-through-a-top-level-binding-names-its-place` shows the quiet
and the re-read forms, whole and through an element. The 13 release
`.wasm` files are byte-identical to adaeaea81's, since no program or std
module holds a collection in a top-level binding, so no study time can
move. A probe that changes lists through bindings (whole, through an
element, a field and a `mut self` call, by an element reference and in a
loop) prints the same and goes from 14,696 to 14,866 B: each `IsUnique`
of a binding's place loads the binding again where the header model
checked the value it had read. Cold build CPU, medians of 11 interleaved
builds with the dev-profile compiler: json 0.35 s to 0.35 s, nbody
0.29 s to 0.29 s. The web compiler's gzip grows 302 B
(1,904,470 to 1,904,772). The known failure
`runtime/valid/a-list-a-callee-stores-at-top-level-is-a-copy` stays: its
change is through a local whose header a callee stored into a binding,
which 3f fixes.

3b copies out a `mut self` receiver of a list or a map, the only
in-place parameter: a `mut xs: List[T]` parameter is the callee's own
copy (`fn.param.mut.copy`), and a `mut List[T]` type cannot be written.
One predicate decides for the callee and every caller
(`emit::copies_out`): the function does not suspend, `OwnCx::received`
answers `InPlace` for its receiver, and the receiver's type at this
instance is the `mut` view of a list or a map. `received` answers
`InPlace` for a receiver declared `mut List`, `mut Map` or `mut Self`,
since a default method's instance may make `Self` a list; insertion then
leaves that receiver unowned, with no `Drop` and no exit row, so no move
or drop empties it before the exit (a returned `self` is read as a copy,
not moved; `own/a-copied-out-receiver-is-never-dropped`). The callee
returns the receiver's value after its results, from its one exit or
each direct return, with a count of its own (one retain), so the value
outlives the callee's release of its parameter, as 3f's replaced buffer
must. A direct or default argument's call stores it into the place the
argument read when that place is rooted at a local or a top-level
binding, by the same lowering as a `Store` of it (`Em::store_into`,
which `place_store` now calls), and releases its count: net, the place
holds the same count of the same header. `DynRead` stores it back
through the reference (`ref_store`, so a view's setter takes it);
`DynSlot`, whose payload is the value itself, and a function value's
adapter have no place and release it. The plan's "no count change" was
written for 3g's convention, where the place's count moves into the
call; with today's retain at the call, a value without a count of its
own would be freed by the callee's exit once 3f replaces the buffer.
A store into memory (an element, a map value, a field in a box) takes
the header copy every such store takes under headers, so
`rows[i].grow(1)` costs one `ListCopy` until 3g
(`calls/a-mut-self-call-copies-the-receiver-out`). No pass needed a
rule for the call as a write: an in-place argument's place has an
`IsUnique` just before the call, which borrow already refuses a borrow
across, and hoisting already counts an argument as a use that keeps the
header, so it neither hoists nor marks that place stable.

Left open, for the owner or later commits: a suspending callee copies
nothing out, since a kept suspension (`let mut s = xs.grow(1)`, then
`race!`) completes away from the call's place; a trait value's payload
(`DynSlot`) and a function value have no place to store into; and in a
default method's body the receiver's calls are `Bound`, so they are not
in-place to the passes (no `IsUnique`, the argument a plain copy) though
the instance copies out. 3d's verifier, which wants every change's
operand to be a read of its place right after its `IsUnique`s, has to
take those calls into account.

3b adds 139 Rust code lines and deletes 19 against 135, under the stop
line of 205 (`emit.rs` +88/−2, `rt.rs` +19/−3, `emit/refs.rs` +14/−12,
`hd_driver` `own.rs` +8/−1, `own/insert.rs` +5/−1, `emit/dyn_value.rs`
+3, `emit/scope.rs` +2). No baseline changes. The 13 release `.wasm`
files are byte-identical to 3a's: nothing in std or the 12 programs has
a `mut self` receiver of a collection. The runtime case
`a-mut-self-change-of-a-list-reaches-the-callers-place` passes before
and after on V8 and after on wasmtime, through a local, an element, a
field, a top-level binding, a reference, a trait value, a default
method, a returned receiver, an early return, a map value and a map. The
web compiler's gzip grows 1,973 B (1,904,772 to 1,906,745, both built
here), against the planned 1.7 KB: 14.2 B a line where the plan used
12.5.

3c stores the marker and tests for it. Before a call that copies out,
at both of 3b's call sites, the caller decides whether the argument's
place is one another path may reach (`Em::reached`): rooted at a
top-level binding or a cell, or past a dereference. If so, it stores
the marker 0 into the place by the `Store` lowering (`Em::mark`), which
releases the place's count while the call's own count of the argument
keeps the value; 3b's copy-back then stores the copied-out value over
the marker, taking a count of its own, and the marker's release is
nothing. Checked-mode conformance caught a first version that retained
the value for the marker's store: one count leaked per call. The marker's store takes no header copy (`Em::marking` in
`fresh`), since `ListCopy` of 0 would read address 0. A read of a list
or a map through such a place tests it (`Em::test_local`, and
`Em::tested` after a read's store): a binding read whole (`global_read`,
`slot_get`), a cell read whole or by a field (`cell_get`, `place_read`),
a read through a dereference (`place_read`, `read_part`, `rooted_read`),
the header before each element step of the walk (`walk_place`), and the
header an `IsUnique` or a `Pin` names (`place_header`), so a push through
the binding panics too. The test is `local.get x`, `call rt:marker_test`
(`Func::marker_test`), which panics `conflicting-access` on 0; it leaves
the anchor state as it was, so the code after it is what it would be
without it. Each marker store also calls `Helper::Marker`, a no-op the
link erases as it erases `CancellationRoot`. Where no function calls it,
the link erases every test with the anchor it wrote, before it builds
any helper (`Module::erase_marker_tests`), so `rt:marker_test` and its
panic text never exist in such a program. That is one scan of the
bodies; the fold the plan named, at `RefViewsAbsent`'s, would come after
the helpers are built, and the panic text's static data would stay.
`hd_check`'s `PANIC_CATEGORIES` gains `conflicting-access`, as
`flow.panic.known-categories` now lists it.

The spec rules are `ref.elem.self-call`, `.store`, `.conflict`, `.paths`
and `.release`, in a new subsection of chapter 13 after Call-Scoped
Access; `types.coll.nested` now says that only a `mut self` method's
call stores a changed copy back, when it returns. The cases are
`runtime/panic/a-read-of-a-binding-during-its-mut-self-call-panics` and
`runtime/valid/a-mut-self-call-on-a-binding-completes-when-nothing-reads-it`,
both passing on V8 and wasmtime, in dev and release; the wat fixture
`calls/a-mut-self-call-on-a-top-level-binding-stores-the-marker` shows
the marker's store, the copy-back over it and the tests. Probes also panic for an
element read, a push and a loop through the binding, a read through a
reference the call was made on, a map's read, and a cell a closure
reads, whole or by a field.

Left open, besides 3b's three: a call through a trait value (`DynRead`)
stores no marker, since the slot helper is built at link time, after
the scan, and its few lines would pass the stop line; a read during it
sees the place's value, as before 3c. A `let mut` list that a closure
captures and nothing reassigns has no cell: the closure holds the same
header, so a read through it sees the change in progress and does not
panic; under 3f the closure's copy would keep the old buffer address,
which 3f has to settle. Only a list or a map read tests: a read of a
struct or a tuple that holds the marked collection copies the marker,
and a later use of that copy reads address 0. A store of the whole
place during the call reads nothing and is not tested, and the
copy-back then overwrites it, so that write is lost; an element store
walks the header and panics.

3c adds 95 Rust code lines and deletes 15 against 68, under the stop
line of 102 (`emit.rs` +28/−1, `emit/refs.rs` +27/−10, `module.rs`
+22/−1, `rt.rs` +14, `hd_check` `tests.rs` +2/−1, `emit/closure.rs` and
`emit/list.rs` +1/−1 each): 1.4 times the share, as threaded emitter
code ran before, the reads alone being ten sites. No baseline changes.
The 13 release `.wasm` files are byte-identical to 3b's: no program
stores a marker, so the link erases every test, and json and
string_build pay nothing as shipped. With the erasure off (a local
patch, not committed), the tests cost: json 169,956 to 172,004 B
(+1.2%), string_build 11,739 to 11,910 B (+1.5%), closures +117 B,
map_count +168 B, nbody +228 B, spectral_norm +187 B, the other seven
unchanged. Study times on V8, `--quick`, medians of three interleaved
rounds of 7 runs, twice: json 121.4 to 123.4 ms and 131.8 to 122.2 ms
(both within the ±2.5% noise, minimums 120.9 to 121.8), string_build
215.8 to 225.2 ms and 216.1 to 229.1 ms (+4.4% and +6.0%): each
`StringBuilder::push` changes `self.parts` through `&mut self`, so the
`IsUnique` and the read of that place test its header, 1.5 million
times each. The build
with the erasure on measured 122.2 and 122.7 ms for json and 215.4 and
220.1 ms for string_build, the same bytes as 3b. The web compiler's
gzip grows 1,075 B (1,906,745 to 1,907,820, both built here).

3d makes every change name its place. One list of the operands an
instruction changes in place (`own::changed`: a push's list, a map
store's or removal's map, each in-place argument) serves insertion and
the verifier, so `uniqueness`'s map arm goes and `m[k] = v` is
`IsUnique [$m]`, `Read borrow [$m]` and the `MapSet`, the read standing
right after the check where nothing between may write and read again
there otherwise, as for a push (`own/a-map-change-names-its-place`). A
read used by more than its change kept the check on its value before;
the one such case in the conformance suite is the map read of
`m[k] += 1`, which also finds the entry and reads the value. It now
keeps its own count for those uses, and the change reads the place again
and no longer counts as one of them (`Tables::reread`). A change through
`[%m [%e]]` whose arguments may write reads the place again with a new
`MapEntry` of the same key after the map's `IsUnique`, one per map value
step on the path: `m[k].push(next(&mut k))` is the receiver's `Read
borrow [$m [%e]]`, which only finds the entry, the call, `IsUnique
[$m]`, `Read borrow [$m]`, `MapEntry` of the key read before the call,
`IsUnique [$m [%e2]]`, `Read borrow [$m [%e2]]` and the push, and the
key's `Drop` follows the push (`Tables::relook`). The emitter lowers a
plain `IsUnique` and a `copy` or `borrow` read of the same place through
a step right after it in its block as one walk: the read takes the
header the check's walk left in a local (`Em::read_walked`). The
verifier's `change_places` wants a change's operand that reads a place
rooted at a local or a binding to read it right after the place's
`IsUnique`, or anywhere a hoisted one names it, with only reads,
arithmetic, coercions, constructions, drops and header copies between
the read and the change. It fired on nothing outside the multiple-use
case, so none of 3b's and 3c's open cases needed an exemption: a
default method's `Bound` calls are not in-place to the passes, a call
through a trait value or a function value is no `Call` the rule reads,
and a suspending callee's `Await` keeps the form.

Left open: an operand that reads no place rooted at a local or a binding
still takes the check on its value, which 3f's write-back cannot store
into, and the verifier skips it. Besides a value-rooted place, that
includes a map value whose index calls something:
`m["k$n"].push(twice(n))` keeps `[%3 [%13]]`, since place folding
stops at the interpolation's call between the map's read and its
`MapEntry`.

3d adds 177 Rust code lines and deletes 61 against 145, under the stop
line of 220 (`own/insert.rs` +62/−54, `ir/owned.rs` +54/−1, `own.rs`
+27, `emit/count.rs` +28/−5, `emit.rs` +6/−1); the verifier's test adds
37 more. Four baselines change as the rule says:
`changes-make-the-collection-unique` and
`a-change-through-a-map-value-names-its-place` (whose push after a call
now looks its entry up again, and which gains the key case), and the wat
of `calls/a-mut-self-call-copies-the-receiver-out` and
`maps/update-from-get`, where the receiver's read takes the check's
header and `counts[word] = ...` borrows the map it read for its entry
where it retained it. The new wat fixture
`element-access/a-change-takes-the-header-its-check-found` shows the one
walk. Release `.wasm` on node against 3c: json 169,956 to 168,515 B
(−0.85%), map_count 18,514 to 18,443 (−0.38%), string_build 11,739 to
11,687 (−0.44%), hello and the other nine byte-identical. Against
6792b1bfa, built here (string_build 11,750, json 172,416, map_count
18,662, hello 7,426), string_build is −0.5%, json −2.3% and map_count
−1.2%: the +4.3% the plan started from was string_build at commit 1
(12,253 B), and later commits shrank it before 3d. Study times on V8,
`--quick`, medians of three interleaved rounds of 7 runs, under a load
average of 8 to 16 from other builds: string_build 221 to 236 ms and
211 to 237 ms, map_count 184 to 188 ms and 207 to 213 ms, json 125 to
128 ms and 131 to 143 ms, with single rounds 50% apart, so noise. Run
by run, 21 interleaved runs each: string_build 218 to 214 ms (CPU 268 to
267 ms), map_count 181 to 181 ms (189 to 188), json 123 to 123 ms (166
to 165). The web compiler's gzip grows 1,884 B (1,907,820 to 1,909,704,
both built here).

3b's copy-out and 3c's marker are removed by the owner's decision of
2026-10-23 on #667: a `mut self` method on a list or a map gets its own
copy of the receiver, exactly as on a struct, so nothing a callee does
to it reaches the caller's place. The commit after 3d reverts both
commits' code (3b's `emit::copies_out` and its callee and caller halves,
the `DynSlot`/`DynRead` `out` flag, insertion's kept receiver and the
`received` answer; 3c's `Em::mark`, the read tests, `rt:marker_test`,
`Helper::Marker`, `Module::erase_marker_tests` and the panic category)
and 3b's `store_into` refactor, which nothing later uses. 3d keeps its
walk and its verifier rule.

The receiver now goes the way a `mut` parameter of a list goes (`mut xs:
List[T]`, `fn.param.mut.copy`): `OwnCx::pass` and `received` answer from
the declared type through `value_parameter`, which reads through the
`mut` view, so the receiver is an owned value argument the move pass
moves in at the caller's last use and that is copied (a retain)
otherwise, and the callee's first change finds it changing and copies it
on entry (insertion's rule 7.5), as
`own/a-mut-self-receiver-is-the-methods-copy` shows. The driver's
`in_place`, which made a receiver declared `mut List`, `mut Map` or (by
the call's receiver type) `mut Self` an in-place argument, was the only
source of `Pass::InPlace`, so the variant goes, with
`OwnCx::in_place_view` (the entry-copy exemption), the call arm of
`own::changed` and of `copies.rs`'s changing locals, rule 7.4's
exemption, borrow's arm and the inliner's `Bind::Place` with its place
rerooting. An entry copy takes the local's type, a receiver's `mut` view
included, so it stores back as read, and the emitter's `value_copy`
copies a list or a map through that view. Only a `mut self` receiver has
a `mut` view of a list or a map, since no written type is one; a
struct's `mut` view, which a `mut` provider has (`std.rt.host_provider`
for json's `std.structure.Member`), stays shared, as before. A list
receiver keeps `OwnCx::receiver`'s exemption from rule 7.4, since the
callee copies it before any change.

This changes later rows. 3b's four rows and 3c's three are gone. 3f's
write-back is unchanged in scope: an `IsUnique`, a push's growth and a
map's put still store a new buffer into the place they name, and
`list_truncate` writes through its reference; no call has an in-place
parameter, so none needs a buffer back from a callee, and the reason
given below for landing copy-out before the buffer address no longer
applies. 3g's row for `OwnCx::header_copy`, `in_place_view` and
`HeaderCopy` deletes about 8 lines fewer, since `in_place_view` went
here. 3g's "a parameter changed in place is owned" row now also covers
`mut self` receivers, which are value parameters like any `mut` one,
at the same size. 3c's open case of a closure's copy keeping an old
buffer address, which 3f had to settle, no longer arises; nor do 3b's
open cases (a kept suspension, a function value, a default method's
`Bound` calls).

The commit adds 73 Rust code lines and deletes 403 (`emit.rs` +3/−116,
`hd_driver` `own.rs` +8/−53, `inline.rs` +5/−51, `emit/refs.rs`
+22/−41, `own/copies.rs` +9/−36, `rt.rs` +3/−33, `module.rs` +1/−22,
`own.rs` +2/−21, `own/insert.rs` +5/−11, `emit/list.rs` +9/−2, the rest
+6/−17); 32 of the 73 restore 3d's parent's lines that 3b and 3c had
edited. The spec drops "Mut Self Calls On Collections" and its five
rules `ref.elem.self-call.*`, puts `types.coll.nested` and
`flow.panic.known-categories` back as before 3c (`types.coll.nested`
now says a `mut self` method on such a path changes only its own copy),
and widens `fn.method.mut-self-struct` and the receiver table's
`fn.method.receiver.mut-self` row to every receiver type, a list or a
map included. The runtime case
`a-mut-self-change-of-a-list-reaches-the-callers-place` becomes
`a-mut-self-change-of-a-list-stays-in-the-methods-copy`, with each of
its places unchanged after a `mut self` call and changed by a `&mut
self` one; 3c's two cases become one,
`a-read-of-a-binding-during-its-mut-self-call-sees-the-unchanged-value`.
Both fail at 3d and pass on V8 and wasmtime, dev and release. The stage
fixtures of 3b and 3c go, and `own/a-mut-self-receiver-changes-its-place`
becomes `own/a-mut-self-receiver-is-the-methods-copy`; no other baseline
changes. The 13 release `.wasm` files are byte-identical to 3d's. The
web compiler's gzip shrinks 4,081 B (1,909,705 to 1,905,624, both built
here).

No commit carries two copy paths: through 3e both kinds copy by
`ListCopy` (a new header over the same buffer), 3f moves both to the
buffer copy glue at once, and 3g removes the header copies of both.
Copy-out (3b, 3c) lands before the buffer address: under headers it
changes nothing observable, and once it is in, a callee that grows a
buffer cannot lose its change.

**Stop for the owner** when a commit's added lines pass its stop line;
when 3g deletes under two thirds of its 1,143 lines, or the component's
last commit has deleted under two thirds of its 2,686; when a bench
program passes +5% of its size at 6792b1bfa (string_build has 84 B of
room today, so 3h is the likeliest); when a judged study time is slower
than the noise; when 3h's or 3j's std code is slower than the kernel it
replaces, which then stays; or when the web gzip comes within 50 KB of
the 2 MB cap, which these numbers do not reach.

**The biggest remaining risk** is 3g's claim that every count a copy of
a collection takes can run the copy glue. Today's count glue retains in
place: `count_locals` retains the leaves in a value's locals, and the
`Retain` and `RetainFields` glue take values and return nothing. A
pinned collection's copy must be a clone, so every such site must store
the glue's result where the copy lives. If some site cannot, such as a
count taken inside erased generic glue or a task frame where the value
is in no writable local or slot, `header_copy` must keep answering for
the types that reach it, and part of `copies.rs` stays: the component
deletes 400 to 900 lines fewer, and stage 3's net falls from about
−1,820 to about −920 to −1,420. The next risks are map_count's size and
time with the map table in hd (3j), and the element glue of the
generic slow paths (3h).

### Stage 3's net

The landed commits at their measured sizes, the `List` and `Map`
buffers at their shares, and the remaining components at their planned
additions times the same 1.3, since each changes existing code.

| Component | Status | Rust added | Rust deleted | std hd |
| --- | --- | ---: | ---: | ---: |
| Inliner | done, four commits | 1,002 | 158 | 0 |
| Typed memory primitives | done, two commits | 337 | 77 | 27 |
| `List`'s commits 1 and 2 | done | 127 | 149 | 0 |
| `List` and `Map` buffers, 3a to 3k | planned | 885 | 2,686 | 235 |
| One `for` lowering | planned | 235 | 960 | 20 |
| Strings in std | planned | 40 | 320 | 80 |
| Deleting the intrinsics | planned | 0 | 100 | −15 |
| **Stage 3** | | **2,626** | **4,450** | **347** |

Stage 3 still deletes code: about 1,820 Rust code lines net and +350 of
std hd, against the −2,250 and +420 planned at 6792b1bfa. The landed
commits are +1,082 so far; the rest must delete about 2,900. Against
the old plan, the landed commits came in about 700 lines under their
shares; the `List` and `Map` buffers net about 990 lines less than the
two rows they replace, since they add 885 against 290 (copy-out and its
check, places at top-level bindings, the write-backs and the copy glue
the old rows did not price, and the calibration) and delete 2,690
against 3,100 (option B keeps the checker's `len` and `push`, and
hoisting's stable lists stay); and deleting the intrinsics is 150
smaller under option B. Without the calibration (+270 on what is left)
the net is about −2,100. If 3g keeps part of `copies.rs`, it is about −920 to
−1,420; if maps keep their Rust glue because std's is slower (3j, 3k),
about −780, still a deletion.

### Compile time and code size

Counted with the `own` view (`Host::render_tir` with the `*` prefix) of
every body the program reaches, after the ownership passes. A call
inlines in the count when its callee is a std body of at most 40
instructions that is no intrinsic and holds no suspension, closure or
provider read; trait calls through a bound or a trait value stay calls.

| | json | nbody | hello (`println("hello")`) |
| --- | ---: | ---: | ---: |
| reached bodies / instructions today | 279 / 10,663 | 76 / 2,724 | 70 / 2,246 |
| intrinsics today | 229: `ListPush` 112, `ListLen` 42, `ValueCopy` 22, `StrIndex` 17, `SnapBegin` and `SnapEnd` 8 each, `StrEq` 6, map 11, `StrConcat` 3 | 6 | 1 |
| small std calls outside `std.rt` today, copied | 202 sites, 2,546 | 2, 6 | 0 |
| intrinsics as inlined std code, net | +3,540 | +290 (six range loops) | 0 |
| callee bodies no longer emitted | −1,140 | 0 | 0 |
| net instructions | +4,900 (+46%) | +290 (+11%) | 0 |
| passes, at 0.49 µs per instruction | +2.4 ms | +0.14 ms | 0 |
| emission, at 2.1 µs per instruction | +10 ms | +0.6 ms | 0 |

The intrinsics' std sizes come from bodies of the same shape in today's
TIR: `ListIter.next` is 40 instructions, `List.pop` 27 and
`string.len` 3, so `push` is taken as 27, `len` 4, a string index 10, a
`for` over a list or a range 37 (`iter` 7 and a `next` of 30), a map
operation's wrapper 10, and `StrEq` and `StrConcat` stay calls. The
emission rate is the 28 ms json spent on 13,305 instructions in
[measured today](#measured-today). Checking about 420 more lines of std
hd adds about 3 ms to a cold build (6 µs per line, from 80 ms of Body
over std's 12,936 lines).

Option B changes the json column: `push` and `len` stay lowered, so its
112 `ListPush` and 42 `ListLen` sites are not copied, about 3,040
instructions fewer. json's net is then about +1,860 instructions
(+17%), about 0.9 ms of passes and 3.9 ms of emission; nbody's six range
loops are the `for` row's and do not change.

`std.rt` is the exception. The allocator and the collector make 103
small calls in every program, which would copy 1,214 instructions and
drop 399, +815 net: +36% on hello's 2,246, about 2.5 KB on its 6,880 B
of Wasm. The budget assumes the first version leaves `std.rt` callees
alone; the emitter's count glue already inlines the counts' fast paths
(`inline_counts`).

**hello.** `println` reaches `to_string`, which inlines (it is the
string itself), the `Interp` kernel, which stays in the emitter, and
`Console.write` through a trait value, which stays a call. So hello's
Wasm does not change. Were `Interp` lowered to std's two-part concat it
would gain one function, about 100 B; through a list and `concat_all`
it would pull in `push`, growth and the buffer's drop, about 600 B.

**Web compiler.** Two windows of #49's gzip measurements, with every
Rust code line of the web compiler's 18 crates counted at each end,
give the cost of a line: 1e30a80b5^ to 2bdc507c3 added 13,136 Rust code
lines and 1,024 std hd lines for +166,409 B, and 2bdc507c3 to
62b833aa5 removed 2,446 Rust lines for −33,265 B. The embedded std text
compresses to 8.2 B per hd line (its gzip grew 8,377 B in the first
window), which leaves 12.0 and 13.6 B per Rust code line; the whole
stage 2 window gives 10.9. Stage 3 so far took the gzip from 1,884,286 B
to 1,904,048 B (after `List`'s commit 2), mostly the inliner. At 11 to
14 B, what is left, about −2,900 Rust lines and +320 hd lines
([stage 3's net](#stage-3s-net)), is −32 to −41 KB and +2.6 KB: the
end of stage 3 at about 1,866,000 to 1,875,000 B, about 10 to 19 KB
under where it started.

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
| TIR instructions | -15% to -18%: closure forms, hidden `$ref`/`$serial` locals and their stores, rebuild chains | another -8% to -12%: `LocalGet` + `Field` chains become one `Read`; `extra` grows about 5% for place records | counted: json +17% with option B (+46% before it), nbody +11%, hello 0 in reached bodies after inlining; checked bodies about the same |
| TIR wire bytes | -15% (fewer instructions and fewer closure types in the per-body tables) | -5% | std grows by its new hd code |
| check (Body) | -1 to -3 ms of 80: building closure forms is a small part of checking | -1 ms (no copy insertion, no pin bookkeeping) | about +2 ms cold: about 350 more lines of std to check |
| ModuleFinish | -3 ms (wire writing) | -1 ms | +2 ms |
| emission | unchanged | about unchanged: `live::plan` and the loans (4.5 ms on json) go, the passes (estimated 4 to 6 ms on 231 bodies) run at decode instead | json about +4 ms with option B, nbody +0.6 ms: inlined code |
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
| `hd_tir` | +100 | +1,550 | −10: the inliner, pointer places and the fold, less `copies.rs` |
| `hd_check` | -650 | -550 | −370 |
| `hd_wasm_linear` | -200 | -1,850 | −1,630 |
| `hd_mono`, `hd_driver`, `hd_structure`, `hd_host_abi` | -20 | +120 | +190, with `hd_run`'s inlined frames |
| Rust, net | -770 | -730 | −1,820 code lines, tests apart |
| `lib/std` (hd), net | 0 | 0 | +350 |

The web compiler is 5.21 MB raw against its 5.25 MB gate. Its crates
hold about 135,000 lines of Rust, and it embeds std's 700 KB of hd
source, so a Rust line costs about 33 bytes and an hd line about 40.
That gives about -25 KB after stage 1 and -24 KB after stage 2; stage
2 in fact added about 137 KB of gzip (#49). The stage 3 column is
[stage 3's net](#stage-3s-net) by crate: the landed commits measured per
commit, the rest at their shares. Its gzip cost uses the measured 11 to
14 B per Rust code line and 8.2 B per std hd line of [compile time and
code size](#compile-time-and-code-size): about 10 to 19 KB under where
stage 3 started, after about +20 KB so far.

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
| a mutable list reference reads through a getter | stage 1, stage 2, stage 3 | stage 2 `Read borrow [*$0]`, `ListLen`; stage 3 `Read borrow [*$0]` and `ListLen` over the buffer address, one load less (option B) |
| a nested field write rebuilds each enclosing value | stage 1 | `Store [$1 .body .x] 1.0` |
| a range loop uses the cursor protocol | stage 3 | the one lowering through `RangeIter`, inlined; the bound test once (#711 handles its bounds checks) |
| a referenced struct field write rebuilds its value | stage 1 | `Store [*$0 .x] 1.0` |
| a user iterable loop uses the cursor protocol | stage 1 (closure form) | `Ref mut [$1]` for the cursor; already the one lowering |
| an element method call unpins after the call | stage 1, stage 2 | `IsUnique`, `Pin`, `Ref [*$0 [%1]]`, the call; the reference temporary's drop after the call |
| copying a struct containing a list uses `ValueCopy` | stage 2 | `Read move [$0]` at the parameter's last use: no count |
| list indexing is an opaque intrinsic | stage 2 | `Read [$0 [%1]]`: bounds check, address, load |
| list length is an opaque intrinsic | stays, by option B | `ListLen` loads the buffer's length word; a declaration-level `@inline` is the first option if that falls short |
| list push is an opaque intrinsic | stays, by option B | `IsUnique` of the place, the lowered push over the buffer address, and a call of std's growth only when the buffer is full |
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
  and lists and maps change to buffers together, commit by commit,
  with bench/study before and after each.
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
  access and loop step is a call otherwise. Rust and Swift inline
  library collections across functions; Go lowers slices in its
  compiler and calls its runtime for maps. Owner, 2026-10-23: the
  inliner is approved, so hd follows Rust and Swift. Alternative: keep the
  list and string intrinsics, and stop after stage 2. Cost: the
  about 4,450 lines stage 3 deletes (about 2,500 of them in the emitter) and the
  list header stay, and so does the
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
