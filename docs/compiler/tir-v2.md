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
after the inliner lands and the ownership passes are measured. The
inliner itself is decided once stage 2 is measured.

- **Typed memory primitives** in a privileged std module: a pointer type
  over `T` with `size_of[T]`, `read`, `write` and `offset`, counted like
  `T`, next to today's untyped `std.rt.memory` loads and stores. This is
  a language-tier intrinsic family, approved by the owner on 2026-10-21;
  its signatures are plain hd declarations, and only their bodies are
  the compiler's.
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
