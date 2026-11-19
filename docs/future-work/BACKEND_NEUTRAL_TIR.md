# Backend-Neutral TIR: Lowered TIR Is All A Backend Reads

Status: accepted design (owner, 2026-11-18: "accept all your
recommendations"); domain: compiler internals; partly built (2026-11-19): steps 0, 2a and 6, and most of step 1. It replaces
`INSTANCE_TIR.md` (git history at 421bcec83; accepted 2026-11-17, built through
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
[Cost](#cost), re-priced from the call graph at ed579e47b):

| Tree | Today | End state | Change |
| --- | --- | --- | --- |
| `crates/hd_wasm/src` | 72,379 | about 57,760 | −14,620 |
| `crates/hd_tir/src` | 47,848 | about 54,980, plus step 3b's lifting | +7,130 |
| `crates/hd_driver/src/own.rs` | 4,234 | about 3,670 | −570 |
| a new crate, `hd_layout`, and `hd_opt` growth | 0 | about 2,880 | +2,880 |
| **All four** | **124,461** | **about 119,280**, plus step 3b's lifting | **−5,185** |

## Open questions

None are open. The owner decided all seven as recommended on 2026-11-18.

1. `Phase::Lowered` is the only input of a backend, checked by a
   verifier. Dev module bytes may change at a step; behaviour and bench
   gates replace byte identity.
2. `hd_tir` may grow by about 7,400 lines while `hd_wasm` shrinks by about
   19,900. The loan analysis and the state machine are language logic.
3. A value of more than 16 scalars lives in memory in every position,
   enums included, so no layout lists leaves. Gated on bench time and
   size.
4. Per-type count and drop glue becomes TIR shim bodies, and the `Walk`
   recipe IR is deleted.
5. The runtime's bodies (15,994 lines generated as Wasm) stay per-backend
   for now. `List` and `Map` stay built-in (owner, 2026-11-12). The
   runtime's source language is decided when LLVM is scheduled.
6. Dev and release share one pass order: generic folds and inlining once
   per body, then per-instance optimization in release only. It lands in
   the last step, gated on the release bench.
7. The per-instance store (#2106) holds the lowered form in place of the
   owned form, so a warm build skips lowering too.

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
4,096 leaves were owned as generic code (`MAX_INSTANCE_LEAVES`, deleted in
3c229234e once a value over 16 scalars stayed an address). The driver's
passes called 21 layout functions of `hd_wasm`; since 377945c23 they read
`hd_layout`.

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
| a captured variable | a local of type `*mut (T, flags)`, made by a `Box` instruction (one counted allocation) | a pointer and a runtime allocation |
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

Steps 0 to 2a are built. Steps 3 to 8 are re-priced from a resolved
call graph at ed579e47b (lane 211; Appendix E, "Call graph"): deletions
are measured non-test spans, additions are estimates with the method in
Appendix E. "Moved" lines change crate and are not counted as deleted.
Each step deletes the path it replaces. Rows are in landing order.

| Step | Adds, and deletes | `hd_wasm` | `hd_tir` | Elsewhere | Net |
| --- | --- | --- | --- | --- | --- |
| 0 | #2166: the owned verifier's re-proofs (lane 204, done) | 0 | −365 | 0 | −365 |
| 1 | `hd_layout`; the leaf fallback, the layout memo scopes, the driver's layout calls into `hd_wasm` (done) | −3,000 | −100 | `hd_layout` +2,950; `own.rs` −500 | −650 |
| 2a | cancel blocks: each await gets a cancel block (rustc's `Yield { drop }`); `own::known` writes `Release` statements into it; `unwind_drops` and `release_unwound` go (lane 210, done) | −17 | +61 | −1 | +43 |
| 6 | index guards as an optimization (done, #2175): the list proofs in `hd_opt::prove`, `emit/guards.rs` deleted, the list caches as TIR values; `stable_lists` stays as the region rows | −3,608 | +771 | `hd_opt` +1,251; others +87 | −1,499, measured |
| 5 | loan statements; `excl.rs`, `borrow.rs`, `loan_facts.rs`, `ref_slots.rs`, the loan scopes in `scope.rs`, the loan reads in `count.rs` | −5,408 | +4,390 | 0 | −1,018 |
| 4 | count lowering and shims, keys for shim bodies; `walk_plan`, `Walk`, `ReleasePlan`, the count decisions in `count.rs` | −401 | +850 | `hd_layout` −1,538 | −1,089 |
| 3 | storage lowering and drop elaboration, a `Box` tag; the emitter's cells, `promote.rs`, both `drop_flags.rs` passes, the flag tags | −1,150 | +267 | 0 | −883 |
| 3b | generic local functions lifted as Swift does (Appendix B, "Verified"); `ir/local_fns.rs` deleted | 0 | −601, plus the lifting | 0 | priced when step 3 lands |
| 2b | scope lowering (`Scope`/`Defer` to flag locals and blocks); `emit/scope.rs` | −228 | +245 | 0 | +17 |
| 7 | coroutine lowering, keys for poll and destroy bodies; `susp.rs`, `susp_spill.rs`, `frame_referents.rs`, the suspension count readers left in `count.rs`, and the adapters steps 2b, 4 and 5 leave | −2,505 | +1,600 | 0 | −905 |
| 8 | `verify_lowered`; read modes in the backend, the dev and release split | −72 | +250 | `own.rs` −66 | +112 |
| | **Total**, before 3b's lifting | **−14,622** | **+7,127** | **+2,310** | **−5,185** |

**Where it grows, and why.** Step 8 grows by about 112 lines and step
2b by up to 62 (range −28 to +62). Step 8's old −450 counted
`body_func`'s analysis calls, which steps 3, 4, 5 and 7 delete with
their callees (28 references), and the 20 of 44 backend `ReadMode`
references that sit in files steps 3 to 6 delete. What is left is 24
`ReadMode` references against a 250-line verifier. Folding `verify_lowered` into the owned verifier's tag walk as
a phase check (about +80) would make step 8 −58. Step 2b's high end is
lane 209's measured plan for step 2 less what 2a built.

The earlier total, −8,505, was stale: its rows summed to −7,780. The
re-priced total is −5,185. The gap, step by step:

- **Test lines.** Steps 3, 4, 6 and 7 counted whole files with their
  tests; step 5 did not. That is 1,065 lines: `walk_plan*` 586,
  `guards.rs` 258, `frame_referents.rs` 108, `own/drop_flags.rs` 59,
  `susp.rs` 54.
- **Shared files.** `count.rs` serves steps 4, 5 and 7 and keeps 443
  lines of primitives; `scope.rs` serves 2b, 5 and 7; `borrow.rs` holds
  the frame's loan code. As in step 2, the ladder of one step runs
  another step's work. Appendix E lists the split.
- **Suspension counts stay until step 7.** `susp.rs` reads stated counts
  at `FrameDrop`, `Cold` and `Materialize` 11 times. No TIR body holds
  those points until step 7 makes the poll and destroy bodies, so 101
  lines of `count.rs` move from step 4 to step 7.
- **Step 3 reuses what it was to delete.** Elaboration keeps
  `MovePaths` (165), and `settled` keeps `boundary_inputs` (37).
- **Adapters.** +85 lines that step 7 deletes: `frame_referents`
  over layouts (+45, step 4), the cold destructor's loan ends (+30,
  step 5), the cancel exit of a suspension point (+10, step 2b).

Steps 5 and 7 grow `hd_tir` by 5,990 lines and shrink the whole by
1,923. Step 5 moves analysis and rewrites the decisions the emitter makes
while it emits: range −838 to −1,198; the table uses −1,018. Both are
here because an LLVM backend would otherwise rewrite them.

No language rule, intrinsic or syntax is added.

## Appendix A. Stress test

| Case | What happens | Design answer |
| --- | --- | --- |
| A doubling tuple, 13, 20 and 29 levels | today: 5 fallback instances at depth 13; without the fallback, 4.03 s and 7.16 GB (#2102) | 15, 22 and 31 layout entries; `scalars` saturates; the value is `Memory` |
| `Option[D13]`, an enum over that tuple | before part 3a of step 1, `by_address` excluded enums, so the enum's values flattened | an enum over 16 scalars is `Memory`; step 1 adds this, priced at +200 |
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

**Verified on this machine, 2026-11-18** (swiftc 6.4, rustc 1.97.1; the
probes are short programs compiled with the commands shown).

| Claim | Command | What the output shows |
| --- | --- | --- |
| rustc: a drop flag is a `bool` local | `rustc --emit=mir -C opt-level=0` | `let mut _5: bool; _5 = const true; … switchInt(copy _5) -> [0: bb4, otherwise: bb5]` before `drop(_2)` |
| rustc: a local live across an await is a frame field, its flag too | same | `debug s => (((*_17) as variant#3).1: String)`; `(((*_17) as variant#3).3: bool) = const false`; entry `switchInt` on `discriminant((*_17))` |
| Swift: a mutated captured variable is a box | `swiftc -emit-silgen` | `alloc_box ${ var Int }, var, name "count"`; `project_box` |
| Swift: an exclusivity check is an instruction pair | `swiftc -emit-sil` | `begin_access [modify] [dynamic]` … `end_access` |
| Swift: lowered SIL counts are instructions | `swiftc -emit-sil` | `strong_retain`, `strong_release`, `retain_value`, `release_value` |
| Swift: async uses LLVM coroutines | `swiftc -emit-irgen` | `llvm.coro.id.async`, `llvm.coro.suspend.async`; `work` split into `…TQ0_` and `…TY1_` |
| Swift: a generic local function that captures is its own function | `swiftc -emit-silgen` | `// pair #1 <A>(_:) in generic(captured:)`, `sil private … <T> (@in_guaranteed T, @guaranteed String)`: its own SIL function, the capture a parameter. `wrap` in `outer<T>` is `<T><U>`; a mutated capture is `@guaranteed { var Int }`. Calls are direct `function_ref`s. |
| Rust has no generic capturing local function | `rustc` | a nested `fn` that captures: `error[E0434]: can't capture dynamic environment in a fn item`; a closure called at two types: `error[E0308]` |

So `ir/local_fns.rs`'s copies inside the enclosing body follow neither
language. Its module doc's claim that Swift does it is wrong. Step 3b
replaces it with Swift's lifting. Not probed: Swift's `BoundsCheckOpts`
and `AllocBoxToStack`, cited from source only.

**Layout and value representation, measured 2026-11-18.** The probe is a
tuple or struct that doubles per level (`D0 = (i32, i32)`,
`Dn = (Dn-1, Dn-1)`). One `make` function per level builds it, and a
generic `keep` returns it. Depth 13 holds 8,192 `i32`s and depth 20 about
two million. All times are wall-clock builds on this machine.

| Compiler | Depth 13 | Depth 20 | How a large value is represented |
| --- | --- | --- | --- |
| Go 1.27, std cached | 1.2 s | 2.1 s | `ssa.CanSSA` (`cmd/compile/internal/ssa/value.go`): a type stays in registers only with at most `MaxStruct = 4` fields, recursively, and at most 4 pointer words; anything else lives in a stack slot and is copied by one `Move`. `types.CalcSize` runs once per type. |
| rustc 1.97 `-O0` | 1.2 s | 7.8 s | one interned layout per type; a large value is memory, copied by `memcpy` |
| Zig 0.17 Debug, LLVM | 2.2 s | 3.2 s | aggregates by reference through LLVM `-O0`; the self-hosted backend (`-fno-llvm`) times out at depth 20 |
| MoonBit 2026-10-23 | 1.1 s | 1.1 s | every struct is a heap box (`moonbit.gc.malloc(8)` in `make2`), so compilation costs a constant per type but each value allocates |
| rustc `-O3`, Zig ReleaseFast | 2.2 s, 8.5 s | over 120 s | LLVM's optimizer splits aggregates back into scalars |
| swiftc 6.4 `-Onone` | 18.0 s | over 120 s | IRGen splits a value it keeps in registers into a flat element list; function signatures go by address above 32 bytes |
| hd at 026826749, dev, release | 8.4 s, 2.2 s | over 120 s | a flat leaf list per type; dev Wasm at depth 13 is 1.4 MB, with about 115,000 scalar loads against 13 `memory.copy` |

The rules step 1 follows, from that table:

1. **One layout per type**, built from its parts' layouts, never flattened
   (rustc `layout_of`, Go `CalcSize`).
2. **A register-or-memory rule** (Go `CanSSA`): a value lives in Wasm
   locals only if it has at most K fields, recursively, and at most W
   scalars. Otherwise it is `Memory`: in linear memory, passed by address,
   with results through a destination pointer. Go uses 4 fields and 4
   words, Swift 4 words and rustc's ABI 2 scalars; hd uses 16 scalars. K
   and W are chosen on the release bench, at 4, 8 and 16.
3. **One block copy** for a `Memory` value: one `memory.copy` of its size,
   never a loop over its scalars (Go `Move`, rustc `memcpy`).
4. **Build in place** (Zig result locations, rustc `sret`): an aggregate
   whose layout is `Memory` is built in its destination, each field at its
   offset, with no temporary and no copy after.
5. **No scalar splitting past the rule.** The release optimizer replaces
   an aggregate by its scalars only under rule 2. Splitting without that
   bound is what sends LLVM's `-O3` past 120 s.
6. **No boxing.** MoonBit's heap box compiles fast but allocates per
   value, which hd's value semantics and runtime goals rule out.

The target: the depth 20 probe builds in under 10 s in dev and in release,
and its Wasm grows linearly with the source.

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
| `emit/guards.rs`, list caches (done) | which index guards are false; a stable list's header and length | an element step's unchecked bit; the header as a value (the hoisted `IsUnique`'s, or a `Read borrow`) and the length in a local before the loop, named by the step (`hd_opt::prove`); the header-read hoists dropped (#2193) | Go's `prove.go`, `BoundsCheckOpts` | per loop, one walk of its body once its cheap tests pass | 6 |
| `emit/scope.rs`, `unwind_drops`, the destructor's drops in `susp.rs` | the cleanup ladder; what a cancelled point releases | flag locals, an exit-code local, blocks; a cancel block per await | rustc `builder/scope.rs`, `Yield { drop }` | O(n) | 2 |
| `emit/susp.rs`, `susp_spill.rs`, `frame_referents.rs`, `susp` rows | states, frame slots, resume branches, the destructor's shape | frame tuple, poll body, destroy body | rustc `StateTransform` | backward liveness, O(blocks × locals / 64); one rewrite, O(n) | 7 |
| `emit/refs.rs` storage and partial-move branches | how a place is reached per storage kind | one offset fold over the layout | rustc `PlaceRef` | O(projections) | 3 |
| `emit/dyn_value.rs`, `emit/ctx.rs`, `drive.rs` | vtable slots, hidden row parameters | explicit parameters and a neutral vtable table | Swift lowered function types | not priced; no line claimed | after 8 |
| `emit/list.rs`, `emit/map.rs`, `map.rs`, `rt.rs`, `task.rs`, `boundary.rs` | nothing about a program; they are the built-ins' and runtime's bodies | stay; named by a runtime table | — | — | decision 5 |

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
| The 4,096-leaf fallback | deleted in 3c229234e | 1, done |
| Generic-body ownership that serves it | `Env::owned_generated`, `ProgramEnv::supplied_body` (#2102's second comment) | 1 |
| Layout memo scopes | `layout::cached`, `cached_for_run`, `Scope`, 257 lines | 1 |
| A second per-type fact memo | `own.rs` `TypeTable`, `TypeFacts`, 229 lines | 1 |
| Step 4's second body copy | `ir/local_fns.rs`, 809 lines | 3b (#2180) |
| Verifier checks that re-prove a pass | `ir/owned.rs`, deleted (step 0, −365) | 0, done |
| Storage found twice | `closure.rs` `shared_locals`, `cell_locals`; `borrow.rs` `StorageRoots`; `own/storage.rs` | 3 |
| The settle pass hosted in the backend | `promote.rs` `settled` | 3 |
| Two move-path tables | `own/parts.rs`, `own/drop_flags.rs` | 1, 3 |
| Owned-or-lent decided twice (#2144) | the passes and `emit/count.rs` | 4 |
| Count rules in the Wasm peephole | `opt.rs` rules 11 and 16 | 4, then measured |
| Two origin analyses | `own/frame/origins.rs`; `excl.rs` `Origin*` | 5 |
| Value ends found twice | `excl.rs` `ValueEnds`; `own/reason/future.rs` | 5 |
| Write effects found twice | `guards.rs` `Writes` (deleted); `own/memory_effects.rs` | 6, done |
| Flag and loan saves chosen by hand for the destructor | `select_frame_drop_flags`, `select_frame_loan_cells` | 2 |
| Suspension liveness found twice | `susp.rs` `save_area`; `own/frame.rs` | 7 |
| Per-body fact caches for the emitter | `emit/body_facts.rs`, `Em::tir_flags`, `cell_layouts`, `list_alias_memo` | 3, 6 |
| The dev and release split | `Env::inlined_instance`'s `dev` flag, `generic_inlined` | 8 |
| `gap: #2075`, a projection chain past its depth | `hd_mono/src/lib.rs:590` | not on this path; stays with #2075 |

**What instance TIR built.** Its record is in git history at 421bcec83,
`INSTANCE_TIR.md`. Its four decisions stand: no
fallback, exact keys first, recorded reads, client-side instances.

| Step | State | Here |
| --- | --- | --- |
| 1, 3, 4b, 5, 6, 7a, 7b | built (#2101, #2103, #2125, #2105, #2106, 92fc1f83b, 5c188e1c4) | kept |
| 4, generic local functions | built at +955 (96a648c7c) as in-body copies, citing Swift, which does the opposite | step 3b lifts them as Swift does and deletes `local_fns.rs` |
| 2, layout memo | built as `hd_layout`'s `Layouts`; the fallback deleted in 3c229234e | step 1, done |
| 7, flags and counts (#1420, #2107) | part built (2a5236de0, 69fa9f72b) | steps 3 and 4 |
| 6b, recorded reads and a code cache | no open ticket; after #49 point 4 | unchanged; the unit is the lowered body |

## Appendix E. Details

**Step notes.** Each step is a few commits. Each commit deletes the
path it replaces and names its reference implementation. Rows 2b to 8
are measured at ed579e47b in non-test lines (lane 211).

| Step | Measured deletions | Estimated additions and their method |
| --- | --- | --- |
| 0 | the owned verifier's re-proofs: −365 net (lane 204: the checks that re-run their pass's own predicate, and `verify_instance` folded in; `counts::verify` stays while about 30 tests use it) | none |
| 3b | `ir/local_fns.rs` 809 (601 non-test, 208 tests). Folding it onto `ir::copy` measured only about −60 (lane 204), since the copy is already shared | lifting: each generic local function becomes its own body keyed by (enclosing item, sub-body), with the enclosing type parameters plus its own, and its captures as parameters, boxed when mutated (step 3's `Box`). `hd_mono::instantiate` then specializes it like any generic function. Priced by a probe once step 3 lands. |
| 1 | memo scopes 257; `hd_wasm/lib.rs` driver wrappers 257; `own.rs` fallback 73; `TypeTable` 229 and about 2,000 of `layout.rs` and 594 of cycle code move | interner and target +150 (one arena and one map); enum and suspending memory locals +200 (the frame-block path in `indirect.rs` is 180); `own/parts.rs` 459 becomes shared move paths, about 350 |
| 2a | `unwind_drops` and `release_unwound` (lane 209 measured) | each await's cancel block, built with the body as rustc's MIR builder does (`yield(..) -> [resume: bbN, drop: bbM]`, checked with `rustc --emit=mir`), filled by `own::known` |
| 6 | done, measured over the step's commits in `crates/*/src` (test modules in those files included): `hd_wasm` −3,608 (`guards.rs` deleted whole, the alias walk, `list_loop_safe`, the block `Tree`, the list caches); `hd_tir` +771 (the region rows, `Nest`, the element step's payloads); `hd_opt` +1,251 (`prove`); others +87; net −1,499 | the guard proofs did not move at 1:1: `guards.rs` proved no guard on the bench, so its alias proof went unported, and the region rows are the one stability proof. The list caches became TIR values at +135 non-test (`prove` +215, `hd_tir` +49, the driver's `Cx` +8, against −132 in the emitter) |
| 5 | `excl.rs` 2,620; `borrow.rs` 2,525; `loan_facts.rs` 335; `ref_slots.rs` 220; `scope.rs` `loan_scope_begin`/`end` 26 and the call in `scope` 3; `count.rs` `prepare_own_loans` and `input_carriers` 167 and 12 loan lines in its exit code; about 30 kept loan calls | analysis moved to `hd_tir` 3,700 (`excl.rs` and `borrow.rs`'s 1,322 of analysis, less `Origin*`); about 600 lines of decisions made while emitting, rewritten as statement insertion at 0.6 to 1.2, 360 to 720; `LoanBegin` and `Access` 140; backend emission 500 (its check and state code is 206 today); cache key 10; adapter +30: the cold destructor ends its captures' loans by `LoanEnd` emission, replacing 63 lines of `borrow.rs` it calls today, until step 7. Net −838 to −1,198 |
| 4 | `count.rs` 362: its 33 decision functions (475) less 81 of suspension-boundary readers and 20 of `release_all_inner` kept for step 7, less 12 gone with step 5; `walk_plan.rs` 733 and `walk_plan/semantics.rs` 345 (`oracle.rs` is test-only); `Walk`, `WalkKind`, `ReleasePlan`, `ReleaseStep` and their builders in `hd_layout/src/lib.rs` 460; 24 count calls in step 3, 2b and 7 code; about 60 vanishing kept calls (`take`, `settle`, `operation_count`) | shim builder +450 (`ReleasePlan::new`, `walk_one` and `switch_walk_by` total 100; a TIR writer and trace glue around them); count lowering +300; shim keys +100 (the generated-body row below); adapter +45: `frame_referents::leaves` over `Layout::fields`, until step 7. `count.rs` keeps 443 lines of primitives. `Walk`'s 73 users in `rt.rs`, `map.rs`, `task.rs`, `boundary.rs`, `views.rs` and others move onto shim calls at 1:1; their walk interpreters, about 236 lines, may shrink further |
| 3 | `closure.rs` cell functions 364 less 12 gone with step 4; `emit/drop_flags.rs` 473 less 3; `own/drop_flags.rs` 408 less `MovePaths` 165, which elaboration reuses; `promote.rs` 169 less `boundary_inputs` 37, which moves to the driver with `settled`; hook guards in `value.rs` 184; `is_cell` at 26 sites, 78; the cold destructor's flag save, select and reload in `susp.rs` 14; flag tags 60; `Body::storage` and `full_cells` 150 (lane 210) | lane 210's plan: `Box` tag +70, storage lowering +250 (`own/local_refs.rs`, a like rewrite, is 467), elaboration +400 (rustc's `elaborate_drops.rs` less unwinding and arrays), emitter `Box` lowering +80 |
| 2b | `emit/scope.rs` 238: the 2b set 271 less `ready_frame` 24, kept for step 7, 6 gone with step 4 and 3 with step 5 | scope lowering +200 (the 271-line ladder rewritten at 0.75; rustc's drop tree gives each exit its own path) to +290 (lane 209's +330 plan for all of step 2, less 2a's 61); adapter +10: a point's cancel exit branches to its cancel block, until step 7 |
| 7 | `susp.rs` 2,042 (2,058 less 14, 3 and 63 gone with steps 3, 4 and 5, plus the 64 lines of adapters and `ready_frame` that steps 5 and 2b leave there); `count.rs` suspension readers 101; `susp_spill.rs` 148; `frame_referents.rs` 151; `suspension_work.rs` 48; about 15 kept calls | coroutine lowering +1,550 (`susp.rs` has 2,029 non-test lines; rustc's transform is about 2,000); poll and destroy keys +50 |
| 8 | `generic_inlined` 16; 24 `ReadMode` references left in `hd_wasm`, about 3 lines each; `own.rs` 50 | `verify_lowered` +250 (`ir/instance.rs` is 333) |

**Plumbing, counted.** Past steps missed this by 300 to 450 lines.

| Item | Measured cost | This plan |
| --- | --- | --- |
| a new tag | 70 lines: `ir.rs` +35, `owned.rs` +32, `slots.rs` +3 (2a5236de0) | +3 (`LoanBegin`, `Access`, `Box`), −2 (`FlagSet`, `FlagClear`): net +1, about +70; `LoanEnd`, `Retain` and `Release` exist |
| a generated body (poll, destroy, shim) | none today: an instance is a `Node`, an item at type arguments | a key: poll and destroy by owner instance and kind, a shim by structural hash; lowering hands them to collection with the owner's body, and the store keeps them in the owner's lowered entry: about +150 (steps 4 and 7) |
| a new body column | wire +2 to +33, verifier +12 to +96 (3bd6ce84c, f155744f5, e54ccb9f9) | none added; `Await` gains one block word |
| a column removed | about −21 each (6c1358319: two columns, −42) | 6 leave the owned wire section, by decision 7: about −130 |
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
| the emitter's list caches | the header as a TIR value and the length in a local before the loop; `stable_lists` stays as the region rows the proofs read |
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
| Warm dev build, no edit | one key probe per instance | the same; the store holds the lowered form (decision 7) |
| Runtime, Wasm size | baseline | bench gate per step, measured against 421bcec83 so the caps never compound: output equal, size within 0.5%, time within 2% |
| Web compiler gzip (2.6 MB cap) | baseline | smaller: about 5,200 fewer lines linked |
| Accumulated state | no per-instance work scales with it | the same; the never-lent set stays one link-time pass |

**Order of the lowerings.** Scopes run first, so a suite is a block and
each await has its cancel block. Drops run before storage, so a flag is
boxed with its root. Counts and loans run before the coroutine pass, so
the destroy body gets their statements per state. Storage runs before
the coroutine pass, so a box address is a saved local.

The steps land in another order: 6, 5, 4, 3, 3b, 2b, 7, 8. Each step's
old path must be gone when it lands, so the order follows the call graph
below. Step 6 has no caller outside kept code. Step 5 goes before 4 and
3, since `count.rs` calls the loan code and `borrow.rs` the cell code.
Step 4 goes before 3, since `count.rs` calls the cells and drop flags 18
times, while the cell code's 8 count calls vanish once counts are
statements. Step 3b needs step 3's `Box`. Step 2b goes after 5, which
takes the loan scopes, and before 7, since a resume must not enter a
`Scope` in its middle. Step 7 goes after 4, 5 and 2b. A flag may be a
plain local while the emitter writes the destructor: the destructor
releases only a cold frame (a started frame is never freed, #1430), whose
parameters' flags `save_initial_drop_flags` sets to 1, so step 3 deletes
the frame's flag save, select and reload.

**Call graph** (lane 211, at ed579e47b). For each item a step deletes,
its callers, resolved to definitions by rust-analyzer: 621 items, 1,956
edges. An edge conflicts when its caller's step lands later. In the
order above:

| Deleted by | Called from | Refs | Resolution |
| --- | --- | --- | --- |
| 3 (cells, drop flags) | 4, 5, 6 | 30 | reorder: those steps land first and delete the callers |
| 3 (`is_cell`, `new_cell`, the frame's flag save) | 7, `susp.rs` `finish` | 7 | step 3 deletes the calls, 14 lines: a box is a plain counted local, and a cold frame's flags are 1 |
| 4 (count decisions) | 3, the cell and flag code | 5 | step 4 deletes the calls: the count is a statement around the access |
| 4 (`Walk`) | 3, `cell_mem`, `cell_walk`, `drop_inline_flags` | 3 | step 4 rewrites them as shim calls, 1:1 |
| 4 (`settle`, `store_owned`) | 7, the await code | 6 | step 4 deletes or replaces the calls |
| 4 (`stated_count`, `exit_owners`, `count_local`, ...) | 7, `finish`, `count_point_ref` | 18 | kept until step 7: 101 lines of `count.rs` |
| 4 (`Walk`, `WalkKind`) | 7, `frame_referents.rs` | 11 | adapter +45 over `Layout::fields`, deleted by step 7 |
| 4 (`take`, `retain_ref`, `settle_value`) | 2b, `ret_value` | 3 | step 4 deletes the calls |
| 5 (`borrow_*`) | 4, the exit and read code in `count.rs` | 6 | step 5 deletes its 12 lines there |
| 5 (`borrow_scope_end`) | 2b, `scope` | 1 | step 5 deletes the call with the loan scopes |
| 5 (`borrow_abandon`, `own_caps`, ...) | 7, `finish` | 5 | adapter +30 by `LoanEnd` emission, deleted by step 7 |
| 2b (`Exit`, `exit`) | 7, `point` | 3 | adapter +10: branch to the cancel block, deleted by step 7 |
| 7 (`FRAME`, `free_own_frame`, `leave_cancelled`, ...) | 2b, 3, 4, 5 | 18 | none: the callers go first; `ready_frame` (24) and `release_all_inner`'s waker and frame-referent lines (20) stay until step 7 |
| 8 (`ReadMode`) | 3, 4, 5, 6 | 20 | none: step 8 lands last |

In the design's order (3, 3b, 4, 5, 2b, 6, 7, 8), 47 of the 78 edge
groups conflicted. In this order 41 do: 24 are calls the deleting step
removes, 9 are the three adapters and 8 are the readers kept until step
7. Kept callers are rewritten inside their step: 380
references for step 4 (151 to `count.rs`, 73 to `Walk` in `hd_wasm`),
104 for step 3, 56 for step 5.

**Decision ledger.** Each remaining step starts from these counts and
is done when its "zero" rows read zero in both columns. The counters
are the `ledger` feature of `hd_wasm` (`src/ledger.rs`, the `ledger!`
sites in `emit/`), off in every shipped and web build;
`node scripts/ledger.ts` builds a compiler with it and prints every
counter for both corpora: 25 s a pass, plus an incremental cargo build. Bench: the 34 programs of
`bench/study` and `bench/runtime/progs`, `--release`. Stage: the 318
`hd_driver` and `hd_wasm` stage cases with a `main` that build as one
file, dev. Taken at af80d59d7 (2026-11-19). The 4a rows are the
handoff's; the other end states are proposed from the owner's answers
of 2026-11-19 (1: boxes at scope entry; 2: zeroing stays; 3: the state
machine and ownership in TIR, frames and entry points backend ABI;
4: row providers stay markers).

| Step | Counters | Bench | Stage | At the step's end |
| --- | --- | ---: | ---: | --- |
| 4a | `store_as/implicit-retain/Read` | 180 | 3,091 | zero |
| 4a | `store_as/implicit-retain/{Block,If,Match,SwitchTag}` | 22 | 456 | zero |
| 4a | `store_as/implicit-retain/Ref` | 241 | 1,987 | zero |
| 4a | `store_as/implicit-retain/{Call,Coerce}` | 8 | 45 | zero; missing from the handoff's rows |
| 4a | `drop-value/skipped-loan` | 15 | 26 | zero |
| 4a | `count/release/at-none/Some(LastUse)` | 257 | 4,948 | zero |
| 4a | `marker/Retain/Entry/Local` | 4 | 86 | zero |
| 4b, with 3 | `marker/*/*/Captures` | 1 | 92 | zero |
| 4b, with 7 | `marker/*/*/{Child,Waker,Results,Referent}` | 30 | 577 | zero |
| 4b, not this push | `marker/*/*/{Row,Result}`, `marker/MakeUnique/*` | 57 | 908 | stays (answer 4) |
| 3a | `cell_of/lazy-test`, `cell_of/lazy-test-suspending` | 3 | 95 | zero: made at scope entry (answer 1) |
| 3a | `cell_of/made`, `cell_of/made-entry`, `param/moved-into-cell-at-entry` | 306 | 3,842 | zero: the emitter's cell code is gone |
| 3a | `cell-made/{heap,stack-slot,static,susp-frame-slot}` | 170 | 2,381 | zero: the kinds are TIR box kinds |
| 3a | `closure/with-shared-capture` | 1 | 16 | zero if closures become environment values |
| 3c | `memflag/{moved-path,dead}-flag-local`, `memlocal/live-flag` | 0 | 121 | zero: folded into TIR flag locals |
| 3c | `flag-stmt/{cell-slot,local}` | 10 | 11 | stays: the one flag mechanism |
| 3c | `drop-flag-read/{drop,exit-or-unwind,overwrite}` | 2 | 126 | zero only once exit, unwind and overwrite releases are TIR `Drop`s too (lane 225's design doubt) |
| 3c | `partial-move/{zeroed-in-place,out-of-cell-zeroed}`, `drop-after-partial-move/*` | 200 | 556 | stays: zeroing (answer 2) |
| 3c | `partial-move/niche-repairs` | 26 | 96 | stays, read from `hd_layout`'s empty pattern |
| 5 | `scope/loan-ladder` | 0 | 1 | zero |
| 2b | `scope/defer`, `exit/through-ladder` | 1 | 26 | zero |
| 7 | `susp/zero-moved-value-for-cancel` | 23 | 781 | zero: cancel ownership in TIR (answer 3) |
| 7 | `count/release/at-*/Some(Cancelled)` | 6 | 184 | zero |
| 7 | `susp/point/*`, `susp/suspend-site(materialize)`, `susp/await-*`, `susp/finish*`, `susp/own_frame-for-slots` | 34 | 818 | stays: backend ABI (answer 3) |

Step 3b has no counter: its gate is its 70 fixtures. Step 8 starts when
4a's first three rows read zero. Six decisions fire in neither corpus,
so no step can count them down: a TIR partial move whose part has no
place, a partial move of an untracked part, the whole-value glue after
a partial move, a flag statement this instance ignores, a dropped value
whose input was transferred, and a `defer` whose suite waits.

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

**How the numbers were taken**, at 421bcec83:

- totals: `find crates/hd_wasm/src -name '*.rs' | xargs cat | wc -l`,
  and the same for `hd_tir`;
- files: `wc -l`; structure: `ast-grep outline <file>`;
- spans: an item's first doc line to the next item's, test modules
  left out, by a scratch script over the same files;
- sites: `rg -o '\.leaves\b' crates/hd_wasm/src | wc -l`,
  `rg -o 'Tag::' crates/hd_wasm/src | wc -l`, and `rg -c` of a module's
  function names for its callers;
- history: `git show --numstat <commit> -- 'crates/**/*.rs'`;
- the call graph (lane 211, at ed579e47b): `rust-analyzer lsif .`
  (rust-analyzer 1.97.1) resolves each reference to its definition;
  `ast-grep scan` with a rule matching `function_item`, `struct_item`,
  `enum_item`, `const_item`, `impl_item` and `mod_item` gives every
  item's span; a reference's caller is its innermost enclosing `fn`, and
  one inside a test module is dropped; a scratch script joins them with
  each step's deleted items;
- re-priced spans: non-test lines, an item from its first `///` or `#[`
  line to its end, by a scratch script over the `ast-grep` spans.

**Parse log.** The two `text` blocks parse with
`scripts/hd-conformance.sh parse`, and the first passes `hd check`.
Parsing checks syntax only. The `ir`, `rust` and `sil` blocks are not hd.
