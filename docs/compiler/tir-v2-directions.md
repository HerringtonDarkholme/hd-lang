# Replacing TIR: Five Directions

Status: design exploration, 2026-10-21, for issue #667 (closed). The owner
picked Direction 3, designed and built in [tir-v2.md](tir-v2.md); this
document stays as the record of the alternatives.

Under review: TIR ([crates/hd_tir/src/ir.rs](../../crates/hd_tir/src/ir.rs),
[crates/hd_tir/DESIGN.md](../../crates/hd_tir/DESIGN.md) §4.13.11), the
linear backend's layouts, references and loans
([crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
§23.3-23.7, §23.20, §23.27, §23.31, §23.36), the known-bad stage cases
(`crates/hd_check/tests/cases/known-bad-tir/`) and #656's catalog.

## Contents

1. [Problem](#problem)
2. [Fixed constraints](#fixed-constraints)
3. [Survey](#survey)
4. [The three cases every sketch shows](#the-three-cases-every-sketch-shows)
5. [Direction 1: places in today's TIR](#direction-1-places-in-todays-tir)
6. [Direction 2: address values, as in Go](#direction-2-address-values-as-in-go)
7. [Direction 3: place operands with explicit ownership](#direction-3-place-operands-with-explicit-ownership)
8. [Direction 4: Wasm with symbolic offsets from the checker](#direction-4-wasm-with-symbolic-offsets-from-the-checker)
9. [Direction 5: structs always in memory](#direction-5-structs-always-in-memory)
10. [Collections and strings: a choice every direction makes](#collections-and-strings-a-choice-every-direction-makes)
11. [Comparison](#comparison)
12. [Recommendation](#recommendation)
13. [Questions for the owner](#questions-for-the-owner)
14. [Sources](#sources)

## Problem

hd's values behave as Go's: a struct is bytes at an address, a field is
the address plus a constant offset, and a reference is a pointer. TIR
models them as a dynamic language would. The stage cases show it:

| Defect | Evidence today |
| --- | --- |
| References are getter and setter closures | `body.x = 1.0` through `&mut Body` is 41 instructions: two `Closure`s, a `NewData` reference object, a `StrConcat` for its name, `RefPlace`, `RefWrite` |
| Field writes rebuild the value | a nested local write is `CopyData` of `.body`, then `CopyData` of the whole `Nested`; #656 pattern 4 stores every field of `ListIter` per element |
| 24 intrinsics make `List`, `Map` and `string` opaque | `ListLen`, `ListPush`, `ListIndex`, `MapGet`, `StrConcat`, `StrEq`, `ValueCopy`, pins and snapshots each need an emitter arm and a counting rule |
| Ownership is invisible until the emitter | `emit/count.rs`, `emit/live.rs` and three loan kinds decide counts per instance; #656 patterns 1, 2, 6 and 10 are counts no IR pass can see |
| Bases are re-derived per access | `ListPush` takes the list value, so a list needs a header object; nbody reloads `bodies` and re-checks bounds 12 times per pair (#656 pattern 13) |
| Three `for` lowerings plus dead tags | lists use `SnapBegin` and index reads, maps use `MapSlots`, others the `Iterable` cursor; `ForRange`, `ForList`, `ForMap` still exist unused |

The root cause is one fact: TIR has no place a write goes to. Every change
goes through a value, so the value must be a shared object (the list
header), a closure (the reference), or a rebuilt copy (the struct).

## Fixed constraints

- **One IR.** It replaces TIR. The checker emits it, the emitter consumes
  it, and nothing sits between them.
- **Compile time, memory and compiler size count** as much as run time.
  Work is done once per generic body where possible, not per instance.
- **RC on linear memory** with the cycle collector. A reference is one
  address. Value-only recursion is acyclic (#707).
- **Per-function reasoning only**: no whole-program analysis (#702, #703
  held), and no second callee copies (#681).
- **Out of scope**: bounds-check elimination (#711), runtime cycles (#713,
  #715).

## Survey

| System | Places | Counts and copies | Collections | Generic code |
| --- | --- | --- | --- | --- |
| Go `go/ssa` | address values: `Alloc`, `FieldAddr`, `IndexAddr`; `*p` loads, `Store` writes | none (tracing GC) | slice is `(ptr, len, cap)`; `append` and maps are runtime code | dictionaries per GC shape |
| Go `cmd/compile` SSA | `OffPtr`, `PtrIndex`, `Load`, `Store`, `Move`; small structs split into SSA values | none | maps are Swiss tables in `internal/runtime/maps` | same |
| Swift SIL (OSSA) | `struct_element_addr`, `index_addr`, `load`, `store` | explicit `copy_value`, `destroy_value`; `load [copy]` versus `load_borrow` | `Array` is library code; `is_unique` / `begin_cow_mutation` before a write | unspecialized code reads value witness tables |
| Rust MIR | a place is a local plus projections (`Deref`, `Field`, `Index`) | operands `Copy` and `Move`; drops placed by drop elaboration | `Vec` and `HashMap` are library code over raw pointers | generic MIR; offsets only in codegen |
| Lean 4 IR | values only (pure language) | explicit `inc`/`dec` inserted by a pass; borrowed parameters inferred | `Array` with destructive update when unique | specialization |
| Koka (Perceus) | values only | `dup`/`drop` inserted precisely, then fused and reused per function | library code | — |

Takeaways:

1. Every system with value semantics and mutation names the place a write
   goes to. Go and SIL make the address a value; MIR keeps the place
   symbolic until codegen.
2. RC systems (SIL, Lean, Koka) put counts in the IR as instructions and
   remove them with per-function passes. None decides them in the code
   writer.
3. Collections are library code over a few memory primitives in Go, Rust
   and Swift. The compiler knows pointers, not lists.

## The three cases every sketch shows

| Case | Source | Today's TIR |
| --- | --- | --- |
| Field write through `&mut` | `fn update(body: &mut Body): body.x = 1.0` | 41 instructions, a reference object built from two closures |
| List loop | `fn visit(items: List[i32]): for item in items: ...` | 34 instructions: `SnapBegin`, `ListLen`, `ListIndex` per step, `SnapEnd` |
| Struct holding a list, copied | `fn copy(value: Record) -> Record: let mut copied = value; copied` | `Intrinsic ValueCopy`, counts decided in the emitter |

The sketches are not hd code and are not parsed. `T.f` names field `f` of
type `T`; its offset is resolved per instance.

## Direction 1: places in today's TIR

Keep TIR as it is and add one thing: a place operand (a local, a
dereference or a list element, then a field path). `RefPlace` already has
this record; the change deletes the closure forms and lets `LocalSet`,
`FieldSet` and the reference instructions write a place. Counts, intrinsics
and loops stay as they are.

```tir
update:  Store [*$0 .x] 1.0:f64                  # one instruction; no closures
visit:   unchanged: SnapBegin, ListLen, ListIndex, SnapEnd
copy:    %1 = Copy %0 : Record                   # ValueCopy renamed; emitter still counts
```

- **Fixes:** getter references and whole-value rebuilds (#667 items 1-2,
  #656 pattern 4). Nothing else.
- **Cost:** IR shrinks for reference-heavy bodies; `strip_closure_forms`
  and most of `hd_check/src/refs.rs` go. Compile time and size go down a
  little.
- **Cognitive load:** slightly lower; the emitter's count, loan and list
  rules stay.
- **Migration:** about 4-6 tasks.
- **Main risk:** it stops short. The list header, intrinsics and emitter
  ownership remain, so #656 patterns 1, 2, 5, 6, 9 and 13 stay emitter work.

## Direction 2: address values, as in Go

A place becomes an address value at check time: `LocalAddr`, `FieldAddr`,
`IndexAddr`, then `Load` and `Store` of a type. Locals whose address is
never taken stay plain locals, as Go's SSA keeps them in registers.
`FieldAddr` names the field, not its offset, so a generic body stays
symbolic; the emitter resolves the offset per instance. Counts become
instructions (`Copy`, `Destroy`, `Borrow`, as SIL's `copy_value`), placed
conservatively by the checker and removed by per-body passes.

```tir
update(%body: &Body):
  %a = FieldAddr %body Body.x          # offset per instance
  Store %a 1.0
visit(%items: List[i32]):              # after inlining ListIter.next
  %n = Load (FieldAddr %items Buf.len)
  loop
    %i = LocalGet $at
    If (Prim Ge %i %n) Break
    %v = Load (IndexAddr %items %i) : i32
    LocalSet $at (Prim Add %i 1)
copy(%value: Record):
  %c = Copy %value : Record            # retain of value.items' buffer
  Destroy %value                       # the move pass cancels both
  Return %c
```

- **Fixes:** all six defects, given collections in std. Base hoisting is
  value numbering on `%n` and the buffer address (#656 pattern 13).
  Ownership passes cover patterns 1, 2 and 6.
- **Cost:** more instructions per place than Direction 3 (one per
  projection plus the load). A borrow or move pass must trace each
  address back to its root to see aliasing. Per-instance work shrinks to
  emission.
- **Cognitive load:** low for anyone who knows Go or LLVM; addresses are
  explicit everywhere.
- **Migration:** about 20-30 tasks.
- **Main risk:** alias reasoning on address values is harder than on
  symbolic places, and generic bodies carry many address values whose
  layout is unknown until the instance.

## Direction 3: place operands with explicit ownership

A place is a local plus projections (`*`, `.field`, `[index]`), as in MIR,
and it is an operand, not a value. Statements assign to a place. Each
operand says how it reads: `move p`, `copy p` (a retain for a counted
type) or `borrow p` (no count, as SIL's `load_borrow`). `drop p`,
`is_unique p` and `pin p` / `unpin p` are statements. The checker emits
`copy` and scope-end drops conservatively; per-body passes turn copies
into moves at last use and into borrows where the span rule holds.
Control flow stays structured (`Block`, `If`, `Loop`, `Break`), as Wasm
wants.

```tir
update(_0: &mut Body):
  (*_0).x = 1.0                        # base + offset(Body.x), folded per instance
visit(_0: List[i32]):                  # after inlining ListIter.next
  _1 = copy (*_0).len                  # List[T] is a buffer address
  loop
    if _2 >= _1: break
    _3 = copy (*_0).elems[_2]          # bounds check stays explicit (#711)
    _2 = _2 + 1
  drop _0
copy(_0: Record) -> Record:
  _1 = copy _0                         # checker output
  return move _1
  # after the move pass: _1 = move _0; no count at all
```

- **Fixes:** all six defects, given collections in std. Partial moves out
  of a matched temporary (#656 pattern 6) are projections of one place.
  The span-loan rule becomes "no write to an overlapping place in the
  span", a prefix comparison.
- **Cost:** the smallest IR of the five; a field path is one operand.
  Ownership passes read no types, so they run once per generic body.
  Today `live::plan` also reads no types but runs per instance. Per
  instance the emitter folds each projection chain to base plus constant
  offset and expands `copy`/`drop` to count glue, or to nothing for a
  type without counted fields.
- **Cognitive load:** places, three operand modes, `drop`, `is_unique`,
  `pin`. The three loan kinds, views of lists, snapshots and the list
  header leave the emitter.
- **Migration:** about 18-25 tasks. Direction 1 is its first step.
- **Main risk:** the one `for` lowering and std collections need a
  per-body inliner and a fold of a match on a known variant. Until both
  exist, loops over lists get slower.

## Direction 4: Wasm with symbolic offsets from the checker

The radical relocation. The checker emits Wasm-shaped structured code
directly: `i32.load`, `f64.store`, `call`, `br_if`, with immediates that
name layout facts (`@off(Body.x)`, `@size(T)`, `@vals(T)` for the locals a
`T` flattens to). The emitter becomes a relocator: per instance it fills
the immediates and expands generic locals. Counts are inserted by a pass
before flattening, while types are still known.

```tir
update(body: i32):
  local.get body
  f64.const 1.0
  f64.store offset=@off(Body.x)
visit(items: i32):
  loop
    local.get items
    local.get i
    i32.const @size(i32)
    i32.mul
    i32.add
    i32.load offset=@off(Buf.elems)
copy(value: @vals(Record)) -> @vals(Record):
  local.get value.items
  call @retain(Record.items)
```

- **Fixes:** everything in principle; nothing is left to lower.
- **Cost:** IR three to five times larger per body; the checker takes on
  match compilation, closures and layout-dependent code. Per-instance
  work becomes very cheap. Compiler size roughly unchanged: code moves
  from the emitter to the checker.
- **Cognitive load:** high. Checker contributors must know Wasm, layouts
  and counting; type facts are gone once flattened.
- **Migration:** a rewrite, about 35-50 tasks.
- **Main risk:** suspension and erased generics need typed values, which
  this IR has dropped. Ownership passes on flattened Wasm locals lose the
  types that say what is counted.

## Direction 5: structs always in memory

The radical simplification. No aggregate is ever a value: every struct,
enum, tuple and list value lives at an address, in a shadow-stack slot or
a heap object. Locals hold only scalars and addresses. The IR shrinks to
about twenty instructions: address arithmetic, scalar `Load`/`Store`,
`CopyMem T` (bytes plus count glue), `DropMem T`, calls and control flow.
A small per-instance pass promotes slots whose address never escapes into
Wasm locals.

```tir
update(body: addr):
  StoreF64 [body + Body.x] 1.0
visit(items: addr):
  n = LoadU32 [items + Buf.len]
  loop: v = LoadI32 [items + Buf.elems + i * 4]
copy(value: addr, out: addr):
  CopyMem Record out <- value          # the move pass makes it a plain byte copy
```

- **Fixes:** all six defects; there is no value form left to rebuild.
- **Cost:** the smallest instruction set and the simplest emitter. The
  slot promotion pass is per instance, and without it struct-heavy code
  runs through memory.
- **Cognitive load:** the lowest: C's model.
- **Migration:** about 20 tasks, plus an ABI change (structs by address
  instead of Wasm multi-value, §23.3).
- **Main risk:** speed. V8 and wasmtime do not promote linear memory into
  registers, so nbody and points depend on the promotion pass being
  complete. It drops a working property of §23.3 for an uncertain one.

## Collections and strings: a choice every direction makes

Directions 2 to 5 assume the owner's direction on intrinsics: `List`, `Map`
and string operations become std hd over memory primitives. Today
`std.rt.memory` already has untyped `load_u32`, `store_u64`, `copy` and
`fill`, and the allocator is hd. Generic collections need typed access: a
std-only pointer type with `size_of[T]`, `read`, `write` and `offset`,
counted like any `T`. That is a language-tier intrinsic family.

| Position | What the compiler knows | Per-program cost |
| --- | --- | --- |
| Intrinsics (today) | 24 operations, each with an emitter arm and a counting rule | none; Rust code in the compiler |
| Runtime calls (Go maps) | a few `rt:` helpers per type, called from IR | none; Rust code in the compiler |
| Std hd over typed pointers (Rust `Vec`) | pointers and layout facts only | std source checked per build; instances per element type |

With places, the list value can be the buffer address. Its capacity,
length and pin count live in the buffer. The header object exists only
because TIR changes a list through its value (§23.7), so it goes, saving
one object and one load per access.

## Comparison

| | 1. Places in TIR | 2. Go addresses | 3. Place operands | 4. Symbolic Wasm | 5. Structs in memory |
| --- | --- | --- | --- | --- | --- |
| References | address of a place | address value | `&p` of a place | `i32` | `addr` |
| Field write through `&mut` | 1 instruction | 2 | 1 | 3 | 1 |
| Counts in the IR | no | yes | yes | yes, before flattening | yes |
| Ownership passes run | in the emitter, per instance | per body | per body | per body | per body |
| Intrinsics left | 24 | memory primitives | memory primitives | none | memory primitives |
| `for` lowerings | 3 | 1 | 1 | 1 | 1 |
| IR size versus today | slightly smaller | about the same | smaller | 3-5x larger | smaller |
| Compile time | slightly better | better | better | per-instance cheap, checker slower | better, promotion pass per instance |
| Compiler size (estimate) | -1 to -2k lines | -3 to -5k | -4 to -6k | about even | -5 to -7k |
| Run-time risk | none | low | low | low | high (struct code in memory) |
| Cognitive load | slightly lower | lower | lower | higher | lowest |
| Migration (one-hour tasks) | 4-6 | 20-30 | 18-25 | 35-50 | about 20 plus an ABI change |
| Main risk | stops short | alias tracing on addresses | needs an inliner first | loses types | speed |

Compiler size figures are estimates from today's line counts: the
emitter's count, live, list, map, iterator and reference modules are 4,025
lines, its map runtime 882, the checker's reference, pin and collection
modules 2,837. The web compiler is about 5.2 MB for about 150k Rust
lines, so a net cut of 4-6k lines is roughly 130-200 KB. Std collections
add hd source text the web compiler embeds, which offsets part of that.
Build working set and the check/collect/emit split are not measured yet;
the full design measures them on `bench/study/json.hd`.

## Recommendation

**Recommendation:** Direction 3, place operands with explicit ownership,
with Direction 1 as its first milestone.

- TIR's `RefPlace` record is already a MIR place (root plus field path).
  The checker produces places today, so the first step deletes code.
- Places stay symbolic in generic bodies at no cost. The emitter folds a
  projection chain to one constant offset per instance, which is today's
  per-instance walk.
- Ownership passes read no types, so they run once per generic body.
  That is the constraint's "work per body, not per instance".
- Overlap of two places is a prefix test. It replaces the emitter's
  parameter, projection and span loans with one rule.
- It keeps structured control flow, multi-value structs (§23.3), the
  suspension lowering and the erased-generic boxes.

It gives up Direction 2's free value numbering of addresses: hoisting a
list's buffer out of a loop needs a small pass that binds the projection
prefix to a temporary. The next best is Direction 2; it fixes the same
defects with harder alias reasoning.

## Questions for the owner

1. **Which direction?** The choice sets the shape of every later task.
   Options: 1 alone, 2, 3, 4 or 5. Recommended: 3, starting with 1. Example:
   `body.x = 1.0` through `&mut` becomes `(*_0).x = 1.0`.
2. **Keep structured control flow?** A CFG (Go, MIR, SIL) needs a
   stackifier pass to reach Wasm and changes the suspension lowering.
   Options: keep `Block`/`If`/`Loop` (recommended), or move to a CFG.
3. **Collections and strings as std hd over a typed pointer?** It adds a
   std-only, language-tier intrinsic family (`size_of[T]`, `read`, `write`,
   `offset`) and removes the 24 operations. Options: std hd (recommended,
   after places land); runtime calls as Go does for maps; keep intrinsics.
   Example: `xs[i] = v` becomes `List.set`, inlined to `is_unique` and one
   store.
4. **List value: buffer address, Go's `(ptr, len, cap)`, or today's
   header?** The address is one word and saves the header object; Go's
   form saves a load for the length but triples the size of every list
   field. Recommended: the buffer address, with length, capacity and pin
   count in the buffer.
5. **#704's tag for interior references: not adopted** (2026-10-26). An
   experiment measured the masks at 6-8% on dyn_iter. #704 shipped instead
   as a release-only whole-program fact per reference target
   (`crates/hd_wasm_linear/src/whole.rs`), with no tag and no masks.

## Sources

- Go `go/ssa`: <https://pkg.go.dev/golang.org/x/tools/go/ssa> (`Alloc`,
  `FieldAddr`, `IndexAddr`, `Store`).
- Go compiler SSA: <https://github.com/golang/go/blob/master/src/cmd/compile/internal/ssa/README.md>
  and `_gen/genericOps.go` (`OffPtr`, `PtrIndex`, `Load`, `Store`, `Move`).
- Go maps as runtime code: <https://go.dev/blog/swisstable>.
- Go generics with dictionaries: <https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md>.
- Swift SIL and ownership SSA: <https://github.com/swiftlang/swift/blob/main/docs/SIL/SIL.md>,
  <https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md>.
- Rust MIR places and drop elaboration: <https://rustc-dev-guide.rust-lang.org/mir/index.html>,
  <https://rustc-dev-guide.rust-lang.org/mir/drop-elaboration.html>.
- Lean 4 IR: Ullrich and de Moura, "Counting Immutable Beans", 2019,
  <https://arxiv.org/abs/1908.05647>.
- Koka Perceus: Reinking et al., PLDI 2021,
  <https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/>.

Parse log: this record has no hd blocks; the `tir` sketches are not hd
and were not parsed.
