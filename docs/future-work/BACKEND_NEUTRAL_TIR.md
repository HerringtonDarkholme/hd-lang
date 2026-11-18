# Backend-Neutral TIR: Lowered TIR Is All A Backend Reads

Status: proposal, 2026-11-18; domain: compiler internals; not accepted.
Nothing here is accepted behavior. It replaces
`docs/future-work/INSTANCE_TIR.md` (accepted 2026-11-17, built through
step 7a) and folds in that design's open steps: #2102, #2107 with #1420,
#2127, #2165 and #2166. Under review: [tir-v2.md](../compiler/tir-v2.md),
[ownership.md](../compiler/ownership.md) and
[crates/hd_wasm/DESIGN.md §23](../../crates/hd_wasm/DESIGN.md#23-linear-memory-layout).
Owner direction, 2026-11-18: "push TIR to the end, this is what swift is
doing", with no hd-specific logic in a backend and no growth.

## The proposal

TIR gets one last phase, `Lowered`, and a backend reads nothing else.
In lowered TIR each decision the Wasm emitter makes today is an ordinary
local, statement or block. A drop flag is a `bool` local under an `If`.
A captured variable is a box. A suspending function is a frame tuple, a
poll body and a destroy body. A loan and a count are statements.

Layout moves to a shared crate, `hd_layout`: one interned layout per type,
as rustc's `rustc_abi`, never a list of scalar leaves. The passes and
every backend read it. A backend keeps the ABI, the runtime's bodies,
instruction selection and the target's memory model.

```text
use std.collections.UniqList
use std.time.{Clock, ms, sleep}

fn wait!(kept: UniqList[string], bag: *mut UniqList[UniqList[string]], early: bool) -> void $ Clock:
    sleep!(ms(1))
    if early:
        bag.push(kept)
```

```ir
# lowered TIR for `wait!` (illustrative text, not a file format)
frame = (TaskPrefix, pc: u32, kept, bag, early, child, kept_live: bool)   # an ordinary tuple

fn wait.poll($f: &mut frame) -> Poll:
  Block L1:
    Block L0:
      Match (Read [*$f .pc]):  0 -> Break L0;  1 -> Break L1   # resume dispatch
    Store [*$f .kept_live] true          # the flag is a field like any other
    Store [*$f .child] (Call sleep.new ..)
    Store [*$f .pc] 1
    Return Pending
  If (Call poll (Read [*$f .child])) is Pending: Return Pending
  If (Read [*$f .early]):
    Call UniqList.push (Read [*$f .bag]) (Read [*$f .kept])    # a move: no count
    Store [*$f .kept_live] false
  If (Read [*$f .kept_live]): Call drop[UniqList[string]] (Ref [*$f .kept])
  Return Ready

fn wait.destroy($f: &mut frame):         # an abandoned frame, and the collector's trace
  If (Read [*$f .kept_live]): Call drop[UniqList[string]] (Ref [*$f .kept])
```

Today the same function needs `FlagSet` and `FlagClear` statements, and
a flag slot the emitter saves and reloads around the await. The emitter
lays out the frame and writes the destructor, which has no TIR body.

**End state, lines** (`wc -l` at 421bcec83; the plan's figures are in
[Cost](#cost)):

| Tree | Today | End state | Change |
| --- | --- | --- | --- |
| `crates/hd_wasm/src` | 72,379 | about 52,500 | −19,900 |
| `crates/hd_tir/src` | 47,848 | about 55,250 | +7,400 |
| `crates/hd_driver/src/own.rs` | 4,234 | about 3,700 | −550 |
| new `crates/hd_layout/src`, and `hd_opt` growth | 0 | about 4,500 | +4,500 |
| **All four** | **124,461** | **about 115,900** | **−8,550** |

## Open questions

1. Is `Phase::Lowered` the only input of a backend, checked by a verifier?
   Dev module bytes then change at most steps; behaviour and bench gates
   replace byte identity. **Recommended: yes.**
2. May `hd_tir` grow by about 7,400 lines while `hd_wasm` shrinks by about
   19,900? The loan analysis and the state machine move; they are
   language logic. **Recommended: yes.**
3. Does a value of more than 16 scalars live in memory in every position,
   enums included, so no layout lists leaves? **Recommended: yes**,
   gated on bench time and size.
4. Does per-type count and drop glue become TIR shim bodies, deleting the
   `Walk` recipe IR? **Recommended: yes.**
5. Do the runtime's bodies (15,994 lines generated as Wasm) stay
   per-backend for now? **Recommended: yes.** `List` and `Map` stay
   built-in (owner, 2026-11-12). The runtime's source language is decided
   when LLVM is scheduled.
6. One pass order for dev and release: generic folds and inlining once
   per body, then per-instance optimization in release only?
   **Recommended: yes**, in the last step, gated on the release bench.
7. Does the per-instance store (#2106) hold the lowered form in place of
   the owned form, so a warm build skips lowering too? **Recommended:
   yes.**

## Problems it solves

**1. A backend decides language semantics.** By `wc -l`, 26% of
`hd_wasm/src` only decides, and another 22% mixes decisions with
lowering (Appendix D). An LLVM backend would have to rewrite the
decision-only part: 18,562 lines of hd rules. They cover loans, drop
flags, cells, suspension, count walks and index proofs. Proposed: those
rules run once, in `hd_tir`, and a backend lowers statements.

**2. A fact has three homes.** A drop flag is a TIR statement, a pass
with its own move-path table (`own/drop_flags.rs`, 467 lines) and an
emitter module (473 lines, called from 27 sites in 7 other files). The emitter module re-reads the
statements into sorted path vectors, lays out cell slots and saves flags
across awaits. Proposed: the flag is a `bool` local. Cells and frames
hold it as they hold any local, so those three jobs have no code.

**3. Layout is flattened, and it is the backend's.** `Mem` keeps one
`Leaf` per scalar, read at 154 places in 19 files (`rg -o '\.leaves\b'`).
A type that doubles per level has exponentially many, so instances past
4,096 leaves are owned as generic code (`MAX_INSTANCE_LEAVES`). The
driver's passes call 21 layout functions of `hd_wasm`.

```text
type D0 = (i32, i32)
type D1 = (D0, D0)
type D2 = (D1, D1)

fn keep[T](x: T) -> T:
    x
```

Proposed: continued to `D13`, the type has 15 layout entries, `i32` and
one per level, and `keep[D13]` is an instance like any other.

**4. Results live in side tables.** Pointer loans are "a link-time result
table, not loan begin/end statements" (`own/pointer.rs`). Call targets
are a map per instance keyed by instruction index (`Collected::calls`).
Storage, exit sets and stable lists are body columns. Each table needs a
reader in every backend, and each column costs wire, verifier, copy and
substitution code. Proposed: the lowered body says it in statements.

## How it works

**The pipeline.** Each arrow is a TIR-to-TIR pass over one instance
body, linear in that body. Nothing after `instantiate` reads a generic
form, and nothing after `lower` reads an ownership form.

```ir
checked TIR ─► instantiate ─► own ─► lower ─► lowered TIR ─► backend
 (generic)     (hd_mono)      (counts decided)   │
                                                 ├ scopes      Scope/Defer → flags and blocks; each await gets a cancel block
                                                 ├ drops       flags → bool locals; Drop → If + call
                                                 ├ counts      read modes → Retain/Release; exit sets → statements
                                                 ├ loans       LoanBegin / LoanEnd / Access statements
                                                 ├ storage     shared locals → boxes; closures → env tuples
                                                 └ coroutine   frame tuple, poll body, destroy body
```

**The contract.** The test for each row: an LLVM backend lowers it
knowing no hd rule.

| In lowered TIR | It is | A backend emits |
| --- | --- | --- |
| a type | concrete, with a `LayoutId` | offsets from the layout |
| a local | a value, or memory when a `Ref` names it | an SSA value or a stack slot |
| a captured variable | a local of type `*mut (T, flags)`, made by `box` | a pointer |
| a closure | a boxed tuple of captures and a code pointer | an indirect call, environment first |
| a drop flag | a `bool` local or field | a branch |
| a count | `Retain v` or `Release v` on one counted address; an aggregate calls its shim | a runtime call or inline header code |
| a loan | `LoanBegin`, `LoanEnd` and `Access` on one object | header bit code |
| a suspending function | plain functions over a frame tuple | plain functions |
| a cleanup scope | flag locals and blocks | branches |
| a call | a callee instance, written in the instruction | a call |
| an index guard | present, or deleted by the optimizer | a branch, or nothing |

Before the coroutine pass, each suspension point names a cancel block,
as rustc's `Yield` names its `drop` target. It holds what cancelling
there must release, so the later lowerings treat it as any block.

`verify_lowered` rejects `Await`, `Scope`, `Defer`, `Drop`, `Closure`, a
read mode, a capture column and every pass-output column: one walk of
the columns, O(instructions).

**Layout.** `hd_layout` holds one table per run and target: size,
alignment, field offsets, variants, niche and count facts per type. A
type is laid out once, from its parts' ids, on an explicit worklist. A
query is one probe. A type of more than 16 scalars is `Memory`: an
address, copied with one bulk copy. Appendix E has the struct.

**Data-structure rules.** A new fact is a local, a statement or a block.
Appendix E lists the four tables that stay, with the reason for each,
and the eight structures this design rejects.

## Cost

Deletions are measured spans at 421bcec83. Additions are estimates;
Appendix E gives each method. "Moved" lines change crate and are not
counted as deleted. Each step deletes the path it replaces.

| Step | Adds, and deletes | `hd_wasm` | `hd_tir` | Elsewhere | Net |
| --- | --- | --- | --- | --- | --- |
| 0 | #2165 and #2166: `local_fns.rs`, the verifier's re-proofs | 0 | −1,000 | 0 | −1,000 |
| 1 | `hd_layout`; the leaf fallback, the layout memo scopes, the driver's layout calls into `hd_wasm` | −3,000 | −100 | `hd_layout` +2,950; `own.rs` −500 | −650 |
| 2 | scope lowering and cancel blocks; `emit/scope.rs`, `unwind_drops` | −400 | +300 | 0 | −100 |
| 3 | storage lowering and drop elaboration; the emitter's cells, `promote.rs`, both `drop_flags.rs` files, the flag tags | −1,550 | +100 | 0 | −1,450 |
| 4 | count lowering and shims; `walk_plan`, `Walk`, most of `emit/count.rs` | −2,950 | +700 | 0 | −2,250 |
| 5 | loan statements; `excl.rs`, `borrow.rs`, `loan_facts.rs`, `ref_slots.rs` | −6,750 | +5,650 | 0 | −1,100 |
| 6 | index guards as an optimization; `emit/guards.rs`, the list caches | −2,100 | −50 | `hd_opt` +1,550 | −600 |
| 7 | coroutine lowering; `susp.rs`, `susp_spill.rs`, `frame_referents.rs` | −2,700 | +1,550 | 0 | −1,150 |
| 8 | `verify_lowered`; read modes in the backend, the dev and release split | −450 | +250 | `own.rs` −50 | −250 |
| | **Total** | **−19,900** | **+7,400** | **+3,950** | **−8,550** |

**Where it grows, and why.** No step grows the compiler. Steps 5 and 7
grow `hd_tir` by 7,200 lines and shrink the whole by 2,250. Step 5 is
mostly a move: its measured floor is −1,100. Step 7 is the least
certain, between −700 and −1,500. Both are here because an LLVM backend
would otherwise rewrite them.

No language rule, intrinsic or syntax is added.

## Appendix A. Stress test

| Case | What happens | Design answer |
| --- | --- | --- |
| A doubling tuple, 13, 20 and 29 levels | today: 5 fallback instances at depth 13; without the fallback, 4.03 s and 7.16 GB (#2102) | 15, 22 and 31 layout entries; `scalars` saturates; the value is `Memory` |
| `Option[D13]`, an enum over that tuple | `by_address` excludes enums, so the enum's values flatten | an enum over 16 scalars is `Memory`; step 1 adds this, priced at +200 |
| A shared variable partly moved, captured by two closures | today: flag slots in the cell layout, one per type node | locals with one capture set share a box; the flag is a tuple field |
| A flag live across an await | today: saved to the frame, reloaded in the destructor | a local live across a point is a frame field; no flag rule |
| The collector traces a started, idle frame (bd99dd806 was held for this) | a parameter moved before the await must be skipped | the trace shim is `Match pc` and `If flag` over frame fields |
| Cancellation while a loan is held | today: `borrow_abandon` in the emitter | `LoanEnd` in the point's cancel block, which becomes that state's arm of the destroy body |
| A `using!` suite that suspends | today: a rung a resume enters, shielded | scope lowering runs first; the suite is a block with ordinary states |
| An index guard through a boxed list | today: a `BOX` origin kind in the proof | after step 3 the header read is a plain `Deref` place |
| Instruction renumbering per instance | lane 198 ran flags on generic bodies, since `Collected::calls` and the line table key by instruction | the callee is written in the `Call`; `anchors` carries the line |
| 33,793 instances (`glues-precede-a-large-function-table`) | one walk plan per shape | one shim per structural key; lowering is linear per body |
| 10,000 nested blocks | native recursion would overflow | every lowering walks `Topology` on an explicit stack |
| Resume into nested control, structured Wasm | today: `br_table` written by the emitter | `Match pc` with `Break` arms, existing tags; LLVM gets a `switch` |
| An unguarded hook call (lane 198: `resources` +101 B) | the guarded helper needed constant-1 flags | an unguarded drop is a call; a guarded one is `If` and the same call |

What it found:

- Enums and suspending bodies need the memory form before the fallback
  can go. Step 1 carries that work.
- Flags must share their root's box. The rule is by capture set, with no
  flag case.
- Cancellation cleanup has no TIR body today. It must be a block before
  a flag can be a plain local, so cancel blocks are step 2.
- Byte identity can't gate steps 2 to 7. Each needs the bench gate.
- Frames are tuples, so a slot is never reused between states. rustc's
  coroutine layout overlaps such fields. hd does worse here, as today.

## Appendix B. Prior art, with code

**rustc: layout.** One interned `LayoutData` per type; `BackendRepr`
says whether a value is immediate or in memory. hd takes the shape and
the query. [rustc_abi](https://github.com/rust-lang/rust/blob/master/compiler/rustc_abi/src/lib.rs),
[layout_of](https://github.com/rust-lang/rust/blob/master/compiler/rustc_ty_utils/src/layout.rs)

```rust
pub struct LayoutData<FieldIdx, VariantIdx> {     // abbreviated
    pub fields: FieldsShape<FieldIdx>,             // Primitive | Union | Array | Arbitrary { offsets, .. }
    pub variants: Variants<FieldIdx, VariantIdx>,  // Empty | Single | Multiple { tag, tag_encoding, variants, .. }
    pub backend_repr: BackendRepr,                 // Scalar | ScalarPair | SimdVector | Memory
    pub largest_niche: Option<Niche>,
    pub align: AbiAlign,
    pub size: Size,
}
```

**rustc: drop elaboration.** A flag is a `bool` temporary, set by plain
assignments, for move paths that are maybe-initialized at a drop. hd
takes it whole. [elaborate_drops.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_transform/src/elaborate_drops.rs),
[move_paths](https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_dataflow/src/move_paths/mod.rs)

```rust
fn create_drop_flag(&mut self, index: MovePathIndex, span: Span) {   // abbreviated
    let patch = &mut self.patch;
    self.drop_flags[index].get_or_insert_with(|| patch.new_temp(self.tcx.types.bool, span));
}
// set_drop_flag: patch.add_assign(loc, Place::from(flag), Rvalue::Use(const bool))
```

**rustc: coroutines.** `StateTransform` saves locals live across a yield
as fields, turns a yield into a discriminant store and a return, and
adds a switch at entry. It also builds the drop shim. Codegen sees plain
functions. hd takes the transform; frames are tuples, with no overlap.
[coroutine/mod.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_transform/src/coroutine/mod.rs),
[coroutine/drop.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_transform/src/coroutine/drop.rs)

```rust
// MIR after the transform (shape)
bb0: { _d = discriminant((*_1)); switchInt(move _d) -> [0: bb1, 3: bb7, otherwise: bb9]; }
bb5: { ((*_1) as variant#3).0 = move _x;        // a live local, saved as a field
       discriminant((*_1)) = 3; return; }       // was: yield
```

**rustc: phases and what a backend keeps.** `MirPhase::Runtime` lists
what each later phase forbids, as `verify_lowered` does. Codegen keeps
`PassMode`, and decides which locals are SSA by scanning for address
uses. [syntax.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_middle/src/mir/syntax.rs),
[callconv](https://github.com/rust-lang/rust/blob/master/compiler/rustc_target/src/callconv/mod.rs),
[analyze.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_codegen_ssa/src/mir/analyze.rs),
[drop shim](https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_transform/src/shim.rs)

**Swift: SIL to the end.** Boxes, access checks and counts are
instructions. Passes rewrite them; IRGen lowers them.
[Instructions.md](https://github.com/swiftlang/swift/blob/main/docs/SIL/Instructions.md)

```sil
%box  = alloc_box ${ var Int }               // a captured variable is an explicit box
%addr = project_box %box, 0
%a    = begin_access [modify] [dynamic] %addr   // the exclusivity check is an instruction pair
store %v to %a
end_access %a
strong_retain %box                           // lowered SIL: counts are instructions
```

| hd mechanism | Swift pass | Source |
| --- | --- | --- |
| loans made static or dropped | `AccessEnforcementSelection`, `AccessEnforcementOpts` | [selection](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/Mandatory/AccessEnforcementSelection.cpp), [opts](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/Transforms/AccessEnforcementOpts.cpp) |
| a frame cell | `AllocBoxToStack` | [source](https://github.com/swiftlang/swift/blob/main/SwiftCompilerSources/Sources/Optimizer/FunctionPasses/AllocBoxToStack.swift) |
| index guards | `BoundsCheckOpts` | [source](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/LoopTransforms/BoundsCheckOpts.cpp) |
| counts as statements | `OwnershipModelEliminator` | [source](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/Mandatory/OwnershipModelEliminator.cpp) |
| aggregate counts per field | `SILLowerAggregateInstrs` | [source](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/Transforms/SILLowerAggregateInstrs.cpp) |
| large values by address | `LoadableByAddress` | [source](https://github.com/swiftlang/swift/blob/main/lib/IRGen/LoadableByAddress.cpp) |

hd differs in one place. Swift lowers async functions in LLVM's
coroutine passes. Wasm has none, so hd follows rustc there.

## Appendix C. Alternatives considered

- **Keep the emitter's analyses and port them to LLVM.** About 19,000
  lines written twice, and every rule change made twice.
- **A second, lower IR between TIR and the backend.** It doubles the
  wire, verifier and view code. One IR with a phase mark is what rustc
  and Swift ship.
- **Finish instance TIR step 7 as planned.** It keeps flags as special
  statements and the emitter's flag module. That is the growth this
  design removes.
- **Keep `Mem` and cap the leaves.** That is the fallback. It keeps
  generic-body ownership alive to serve it.
- **Overlapping frame fields.** Smaller frames, but a frame type no
  longer is a tuple. Deferred until a measurement asks for it.

## Appendix D. Today in hd

**Every module of `crates/hd_wasm/src`**, by `wc -l`. A decision changes
what the program does or which code exists. Lowering does not.

| Class | Modules (lines) | Total | Share |
| --- | --- | --- | --- |
| Decision | `layout.rs` 3,281, `layout/walk_plan*` 1,664, `emit/excl.rs` 3,725, `emit/borrow.rs` 2,811, `emit/susp.rs` 2,112, `emit/guards.rs` 1,948, `emit/drop_flags.rs` 473, `emit/ref_slots.rs` 382, `emit/scope.rs` 358, `emit/loan_facts.rs` 335, `emit/promote.rs` 286, `frame_referents.rs` 259, `cycle.rs` and `cycle_graph.rs` 594, `emit/body_facts.rs` 186, `emit/susp_spill.rs` 148 | 18,562 | 26% |
| Mixed | `emit.rs` 5,576, `emit/refs.rs` 1,890, `emit/indirect.rs` 1,766, `emit/list.rs` 1,587, `emit/count.rs` 1,203, `emit/value.rs` 957, `emit/closure.rs` 782, `emit/map.rs` 719, `emit/dyn_value.rs` 554, `emit/ctx.rs` 490, `drive.rs` 406 | 15,930 | 22% |
| Runtime bodies | `rt.rs` 5,255, `task.rs` 3,925, `boundary.rs` 3,475, `map.rs` 1,493, `scan.rs` 951, `emit/text.rs` 474, `rt/row.rs` 329, `emit/iter.rs` 92 | 15,994 | 22% |
| Backend optimizer | `opt.rs` 7,267, `opt/*` 2,743, `tables.rs` 367 | 10,377 | 14% |
| Pure lowering | `module.rs` 3,599, `lib.rs` 1,084, `wide.rs` 764, `bulk.rs` 738, `emit/switch.rs` 518, `emit/shape.rs` 485, `count_tests.rs` 332, `op.rs` 331, `emit/control.rs` 236, `static_data.rs` 211, `emit/row.rs` 117, `origins.rs` 114, `emit/loan.rs` 77 | 8,606 | 12% |
| Tests and dev views | `tests.rs` 2,058, `views.rs` 382, `work.rs` 222, `count_audit.rs` 200, `suspension_work.rs` 48 | 2,910 | 4% |

Three more signs of decisions in the emitter: its state struct `Em` has
105 fields, the emitter has 828 methods, and `Tag::` is matched 663
times in 25 files. A pure lowering matches each tag about once.

**Each decision and its replacement.** Cost is per instance body of n
instructions unless stated. Source links are in Appendix B.

| Today | Decides | TIR mechanism | Precedent | Data structure, cost | Step |
| --- | --- | --- | --- | --- | --- |
| `layout.rs` `Mem`, 12 thread-local memo maps | size, offsets, niches, counted and drop facts | `hd_layout` | `rustc_abi` | interned table; O(own fields) per type, O(1) per query | 1 |
| `own.rs` leaf fallback | which instances get generic ownership | none; every instance is owned | — | — | 1 |
| `emit/closure.rs` cells, `promote.rs`, `storage` rows | where a shared local lives | box locals, env tuples | `alloc_box`, `AllocBoxToStack` | one scan for capture sets, one splice; O(n) | 3 |
| `emit/drop_flags.rs`, `own/drop_flags.rs` | which hooks are guarded, flag slots, flag saves | `bool` locals, `If` | `ElaborateDrops` | move paths for mentioned places; bitset dataflow, O(blocks × paths / 64) | 3 |
| `emit/count.rs`, `walk_plan`, `Walk`, `ReleasePlan` | which leaves a count walks; entry and exit counts | `Retain`, `Release`, shim bodies | `OwnershipModelEliminator`, rustc drop shim | O(n); one shim per structural key, O(own fields) | 4 |
| `cycle.rs`, `cycle_graph.rs` | which types the collector tracks | a layout fact, `CYCLIC` | — | incremental SCCs over type edges, as today | 1 |
| `emit/excl.rs`, `emit/borrow.rs`, `loan_facts.rs`, `ref_slots.rs`, `own/pointer.rs` table | holders, loan ranges, own-loan counts, which checks stay | `LoanBegin`, `LoanEnd`, `Access`; counters are locals | `begin_access`, `AccessEnforcementOpts` | O(n + holders); one link-time summary over call-graph SCCs | 5 |
| `emit/guards.rs`, list caches, `stable_lists` | which index guards are false; hoisted header reads | an `hd_opt` pass that deletes the `If` and writes a `Read` before the loop | `BoundsCheckOpts` | per loop, under today's `DEPTH` and `READS` caps | 6 |
| `emit/scope.rs`, `unwind_drops`, the destructor's drops in `susp.rs` | the cleanup ladder; what a cancelled point releases | flag locals, an exit-code local, blocks; a cancel block per await | rustc `builder/scope.rs`, `Yield { drop }` | O(n) | 2 |
| `emit/susp.rs`, `susp_spill.rs`, `frame_referents.rs`, `susp` rows | states, frame slots, resume branches, the destructor's shape | frame tuple, poll body, destroy body | rustc `StateTransform` | backward liveness, O(blocks × locals / 64); one rewrite, O(n) | 7 |
| `emit/refs.rs` storage and partial-move branches | how a place is reached per storage kind | one offset fold over the layout | rustc `PlaceRef` | O(projections) | 3 |
| `emit/dyn_value.rs`, `emit/ctx.rs`, `drive.rs` | vtable slots, hidden row parameters | explicit parameters and a neutral vtable table | Swift lowered function types | not priced; no line claimed | after 8 |
| `emit/list.rs`, `emit/map.rs`, `map.rs`, `rt.rs`, `task.rs`, `boundary.rs` | nothing about a program; they are the built-ins' and runtime's bodies | stay; named by a runtime table | — | — | question 5 |

**The 154 `Mem::leaves` reads**, by idiom (`rg` at 421bcec83). The
patterns overlap, so the rows do not sum to 154.

| Idiom | Sites | Becomes |
| --- | --- | --- |
| loop over leaves to load or store each | 41, with 15 local allocations | three helpers: `load`, `store`, `copy` by `LayoutId` |
| `.leaves.len()` | 28 | `Layout::scalars` |
| `leaves[p.first..p.first + p.count]` | 14 | `Layout::fields` and an offset |
| `leaves: m.leaves` copied into a helper key | 17 | a `LayoutId` |
| `Dst::Mem(at, base, &leaves)` | 10 | `Dst::Mem(at, base, LayoutId)` |
| inside `layout.rs` | 13 | the layout builder, moved |

**Dormant and parallel paths.** Each is deleted by the step named.

| Path | Where | Step |
| --- | --- | --- |
| The 4,096-leaf fallback, `gap: #2102` twice | `own.rs`: `MAX_INSTANCE_LEAVES`, `instance_leaves`, `leaf_fallback` | 1 |
| Generic-body ownership that serves it | `Env::owned_generated`, `ProgramEnv::supplied_body` (#2102's second comment) | 1 |
| Layout memo scopes | `layout::cached`, `cached_for_run`, `Scope`, 257 lines | 1 |
| A second per-type fact memo | `own.rs` `TypeTable`, `TypeFacts`, 229 lines | 1 |
| Step 4's second body copy | `ir/local_fns.rs`, 809 lines (#2165) | 0 |
| Verifier checks that re-prove a pass | `ir/owned.rs` (#2166) | 0 |
| Storage found twice | `closure.rs` `shared_locals`, `cell_locals`; `borrow.rs` `StorageRoots`; `own/storage.rs` | 3 |
| The settle pass hosted in the backend | `promote.rs` `settled` | 3 |
| Two move-path tables | `own/parts.rs`, `own/drop_flags.rs` | 1, 3 |
| Owned-or-lent decided twice (#2144) | the passes and `emit/count.rs` | 4 |
| Count rules in the Wasm peephole | `opt.rs` rules 11 and 16 | 4, then measured |
| Two origin analyses | `own/frame/origins.rs`; `excl.rs` `Origin*` | 5 |
| Value ends found twice | `excl.rs` `ValueEnds`; `own/reason/future.rs` | 5 |
| Write effects found twice | `guards.rs` `Writes`; `own/memory_effects.rs` | 6 |
| Flag and loan saves chosen by hand for the destructor | `select_frame_drop_flags`, `select_frame_loan_cells` | 2 |
| Suspension liveness found twice | `susp.rs` `save_area`; `own/frame.rs` | 7 |
| Per-body fact caches for the emitter | `emit/body_facts.rs`, `Em::tir_flags`, `cell_layouts`, `list_alias_memo` | 3, 6 |
| The dev and release split | `Env::inlined_instance`'s `dev` flag, `generic_inlined` | 8 |
| `gap: #2075`, a projection chain past its depth | `hd_mono/src/lib.rs:590` | not on this path; stays with #2075 |

**What instance TIR built.** Its record is in git history at 421bcec83,
`docs/future-work/INSTANCE_TIR.md`. Its four decisions stand: no
fallback, exact keys first, recorded reads, client-side instances.

| Step | State | Here |
| --- | --- | --- |
| 1, 3, 4b, 5, 6, 7a, 7b | built (#2101, #2103, #2125, #2105, #2106, 92fc1f83b, 5c188e1c4) | kept |
| 4, generic local functions | built at +955 (96a648c7c) | step 0 reverts it (#2165) |
| 2, layout memo (#2102) | open | step 1 |
| 7, flags and counts (#1420, #2107) | part built (2a5236de0, 69fa9f72b) | steps 3 and 4 |
| 6b, recorded reads and a code cache (#2127) | open, after #49 point 4 | unchanged; the unit is the lowered body |

## Appendix E. Details

**Step notes.** Each step is a few commits. Each commit deletes the
path it replaces and names its reference implementation.

| Step | Measured deletions | Estimated additions and their method |
| --- | --- | --- |
| 0 | `local_fns.rs` 809 | ticket floors: −600 and −400 |
| 1 | memo scopes 257; `hd_wasm/lib.rs` driver wrappers 257; `own.rs` fallback 73; `TypeTable` 229 and about 2,000 of `layout.rs` and 594 of cycle code move | interner and target +150 (one arena and one map); enum and suspending memory locals +200 (the frame-block path in `indirect.rs` is 180); `own/parts.rs` 459 becomes shared move paths, about 350 |
| 2 | `scope.rs` 358; the hand-picked destructor saves, about 40 | scope lowering and cancel blocks +300 (the emitter's ladder is 308 non-test lines) |
| 3 | cell functions in `closure.rs` 476; `promote.rs` 286 less about 45 of lowering; `emit/drop_flags.rs` 473; `own/drop_flags.rs` 467; hook guards in `value.rs` 178 | storage lowering +250 (`own/local_refs.rs`, a like rewrite, is 467; the analysis exists in `own/storage.rs`); elaboration +400 (rustc's `elaborate_drops.rs` less unwinding and arrays); 55 `is_cell` sites at about 3 lines |
| 4 | `Walk` and `ReleasePlan` 585; `walk_plan*` 1,664 | shim builder +450 (`ReleasePlan::new`, `walk_one` and `switch_walk_by` total 100; a TIR writer and trace glue around them); count lowering +300; `emit/count.rs` keeps about 500 of 1,203 |
| 5 | `loan_facts.rs` 335; `ref_slots.rs` 382; cell and frame coupling in `borrow.rs` 274; `Origin*` types in `excl.rs` 243 | the rest of `excl.rs` and `borrow.rs` moves, about 5,500; the backend keeps about 500 (its check and state code is 206 today) |
| 6 | emitter-side application in `guards.rs` 376; the file's other 1,572 lines move to `hd_opt` | list caches −150 (7 `Em` fields and their readers) |
| 7 | `susp.rs` 2,112; `susp_spill.rs` 148; `frame_referents.rs` 259; `suspension_work.rs` 48 | coroutine lowering +1,600 (`susp.rs` has 2,029 non-test lines; rustc's transform is about 2,000) |
| 8 | `generic_inlined` 17 | `verify_lowered` +250 (`ir/instance.rs` is 333); 34 `ReadMode` sites and `body_func`'s analysis calls, about −450 |

**Plumbing, counted.** Past steps missed this by 300 to 450 lines.

| Item | Measured cost | This plan |
| --- | --- | --- |
| a new tag | 70 lines: `ir.rs` +35, `owned.rs` +32, `slots.rs` +3 (2a5236de0) | +2 (`LoanBegin`, `Access`), −2 (`FlagSet`, `FlagClear`): net 0; `LoanEnd` exists |
| a new body column | wire +2 to +33, verifier +12 to +96 (3bd6ce84c, f155744f5, e54ccb9f9) | none added; `Await` gains one block word |
| a column removed | about −21 each (6c1358319: two columns, −42) | 6 leave the owned wire section, with question 7: about −130 |
| body-copy remap, `hd_mono::instantiate` | `hd_mono/src/instance.rs` +72 in step 4 (96a648c7c) | none: lowering runs after both |
| cache key | one settings word per pass set | the lowering version and the target |
| tests | one counted test per pass | each lowering: width 1,000 and depth 20, counting visits |

**Data-structure rules.** A new fact is a local, a statement or a block
unless a row below says why not.

| Kept as a table | Why |
| --- | --- |
| layouts, by type | a query result shared by all bodies; rustc's `layout_of` |
| callee summaries, the never-lent type set | facts about other bodies; a body records which it read |
| `syn`, `anchors`, inline origins | debug data; they change no code |
| move paths, per body, during lowering | an index, dropped with the pass; rustc's `MoveData` |

| Rejected | Replaced by |
| --- | --- |
| `FlagSet`, `FlagClear`, `CellLayout`, flag save and reload | `Store` to a `bool` local |
| `Body::storage`, `full_cells`, `cap_mode`, `is_cell` branches | a box local and a `Deref` projection |
| `exit_drops`, `unwind_drops` | `Release` statements at the exit and in each cancel block |
| `stable_lists`, the emitter's list caches | a `Read` into a local before the loop |
| the loan result table, `loan_facts.rs` | loan statements present or absent |
| `Walk`, `ReleasePlan`, reference-slot plans | shim bodies, keyed by a structural hash |
| `Collected::calls` lookups | the callee written in the `Call` |
| `Mem::leaves` | `Layout::fields` and offsets |

Lowering runs after instantiation, the inliner and the check cache. So
a lowered-only fact needs no body-copy remap and no substitution. It
needs a verifier rule and, for a new tag, the tag row and its wire form.

**Layout.** `hd_layout` holds one table per run and target.

```rust
pub struct Layout {                 // rustc_abi::LayoutData
    pub size: u32, pub align: u32,
    pub scalars: u32,               // saturating count; never a list
    pub repr: Repr,                 // Scalars (at most 16) | Memory
    pub fields: Fields,             // Primitive | Record { offsets, tys }
    pub variants: Variants,         // Single | Tagged { tag, enc, variants: Box<[LayoutId]> }
    pub niche: Option<Niche>,
    pub facts: Facts,               // COUNTED | HOLDS_DROP | HOLDS_LOCAL_REF | CYCLIC
}
pub struct Layouts { by_ty: HashMap<Ty, LayoutId>, data: Vec<Layout>, target: Target }
```

A type is laid out once, from its parts' ids, on an explicit worklist:
O(its own fields), with no native recursion. A query is one probe. A
backend asks for the scalars of a `Scalars` type only, at most 16. A
`Memory` value is an address, copied with one bulk copy.

**Other costs.**

| Component | Today | Proposed |
| --- | --- | --- |
| Compile time per instance | the passes, then the emitter's own loan, guard, storage, flag and suspension analyses | the passes, then six linear lowerings; no emitter analysis |
| Layout work | per scalar leaf; exponential for a doubling type | per distinct type; one probe per query |
| Warm dev build, no edit | one key probe per instance | the same; the store holds the lowered form (question 7) |
| Runtime, Wasm size | baseline | bench gate per step: output equal, size within 0.5%, time within 2% |
| Web compiler gzip (2.6 MB cap) | baseline | smaller: about 8,500 fewer lines linked |
| Accumulated state | no per-instance work scales with it | the same; the never-lent set stays one link-time pass |

**Order of the lowerings.** Scopes run first, so a suite is a block and
each await has its cancel block. Drops run before storage, so a flag is
boxed with its root. Counts and loans run before the coroutine pass, so
the destroy body gets their statements per state. Storage runs before
the coroutine pass, so a box address is a saved local.

The steps land in another order. Each step's old path must be gone when
it lands, and a flag can't be a plain local while the emitter still
writes the destructor's drops.

**Checklist.**

- **Pipeline position.** After `own`, before the backend. Invariant:
  the backend's `Em::new` asserts `Phase::Lowered`.
- **Worst case.** Linear in the instance body for each lowering. The
  drop dataflow adds blocks × paths / 64. No native recursion.
- **Caching and determinism.** Lowering is a function of the owned body,
  the layouts it read and the never-lent set. It records those reads
  (#2127). Shim keys are structural hashes.
- **Budgets.** A frame field per live local; a flag per maybe-moved path
  only. Before the flag pass, 746 flag locals served 23 partial updates
  (#1420, lane 197).
- **Tests.** One stage case per contract row, with the `own` view
  printing lowered TIR. One counted test per lowering.
- **Rollback.** Each step is its own commits and reverts alone. Steps 2
  to 7 change dev bytes, so each lands with its bench table.

**Left for the first code step.** Nine source files cite
`docs/future-work/INSTANCE_TIR.md` in comments (`rg -l INSTANCE_TIR
crates`). This pull request changes no code, so step 0 retargets them.

**How the numbers were taken**, at 421bcec83:

- totals: `find crates/hd_wasm/src -name '*.rs' | xargs cat | wc -l`,
  and the same for `hd_tir`;
- files: `wc -l`; structure: `ast-grep outline <file>`;
- spans: an item's first doc line to the next item's, test modules
  left out, by a scratch script over the same files;
- sites: `rg -o '\.leaves\b' crates/hd_wasm/src | wc -l`,
  `rg -o 'Tag::' crates/hd_wasm/src | wc -l`, and `rg -c` of a module's
  function names for its callers;
- history: `git show --numstat <commit> -- 'crates/**/*.rs'`.

**Parse log.** The two `text` blocks parse with
`scripts/hd-conformance.sh parse`, and the first passes `hd check`.
Parsing checks syntax only. The `ir`, `rust` and `sil` blocks are not hd.
