# Runtime Layout And Emitted Wasm Review, 2026-11-13

Status: an audit log. Nothing here is accepted behavior or an owner
decision. Every direction is a proposal for the owner. Each claim is
marked **[V]** (verified in code, by a run, or by a measurement) or
**[I]** (inferred).

## 1. Scope And Method

| Item | Value |
| --- | --- |
| Source | local `main` at `a45fdfc17`; §5.3 at `101151bf0` |
| Binary | the installed `hd` (`~/.cargo/bin/hd`), default engine |
| Machine | Apple arm64, macOS 26 |
| Layout record | `crates/hd_wasm/DESIGN.md` §15 and §23.1 to §23.14, `docs/future-work/ENUM-LAYOUT.md` (git history at 7ccbb4ea8; its decisions are now in §23.14) |
| Layout code | `crates/hd_wasm/src/layout.rs` (outline and enum parts), `lib/std/rt/alloc.hd` (in full), `rt.rs` `list_buf`, `list_grow` and the string append path |
| WAT snapshots | the 483 `.wat` baselines under `crates/hd_wasm/tests/baselines/` and `crates/hd_driver/tests/baselines/`, reviewed by one agent; its top claims rechecked by me (§4) |

Timings are wall time of one `hd FILE` run, compile included, so only
the growth between sizes is meaningful.

## 2. Summary

The value layout is good and in places better than the references. One
doc conflict is left:

| # | Finding | Severity | Fix cost |
| --- | --- | --- | --- |
| L4 | §23.8 says a `dyn` value's data may be "a box of it"; §23.19 says trait values are references with no box. | doc only | trivial |

The emitted Wasm has no careless code at the small scale. Its gaps are
optimizer capabilities that LLVM and Swift have and `hd_wasm`'s `opt.rs`
lacks: facts about memory, about where a value came from, and about
count pairs after inlining (W1 to W8, §4).

The other 1,062 snapshots (§5) show the layout matching rustc's, and
nothing unsound in ownership. Their largest cost sits upstream of the
Wasm. By-value parameters that are only lent are judged owned, which
costs a retain and a release per call: 331 parameters in 120 files
(S1). At least 16 snapshots no longer meet their own `expected:` line
(S8).

## 3. Layout

### 3.1 What Is Well Designed

Each row is compared with what production runtimes ship.

| Choice | hd | Reference | Verdict |
| --- | --- | --- | --- |
| Object header | 8 bytes: a 32-bit count and one descriptor word [V] | Swift 16 (isa and count), Rust `Rc` 16 (strong and weak), Lean 4 and Koka 8 | as small as the best; a 32-bit count cannot overflow on wasm32, since 2^32 references need 16 GiB |
| Struct fields | rustc's `univariant`: largest alignment first, inline, no box [V] | rustc | equal |
| Enums | rustc's `layout_of_enum`, with niches in `bool`, `char`, tags and non-null addresses; `i32??` is 8 bytes [V] | rustc | equal; nested matches branch once on the shared niche cell, as rustc's `SwitchInt` does |
| `T?` of a reference | a nullable address, 4 bytes [V] | Rust `Option<&T>`, Swift `T?` of a class | equal |
| Closures | `(code, env)`, with env 0 when nothing is captured [V] | Rust `dyn Fn` fat pointer, Swift thick function | equal |
| Trait values | `(data, vtable)`, with a static vtable whose supertrait vtables follow the method slots [V] | Rust `dyn` | equal; no existential box |
| Strings | `(address, length)` views of UTF-8, with one static empty string; append reuses the block's slack (`rt.rs` reads `block_bytes`) [V] | Rust `&str` and `String` | good; there is no small-string inline form (Swift has 15 bytes inline), which is fine for a counted heap |
| Lists and maps | the value is one address of a counted buffer with capacity, length and owner word; copy on write while unshared [V] | Swift `Array`, which also keeps count and capacity in the heap header | equal |
| Immortal statics | count word 0 below the heap, so `retain` is one compare [V] | Swift's immortal refcount, CPython 3.12 immortal objects | equal |
| Shared-loan count overflow | 5 bits in the header, spilling to a side table at 31 (§23.20) [V] | none needed in Rust (static) | handled |

Interior references cost a page-table lookup and a reciprocal multiply on
each count change (§23.5). Neither Rust nor Swift counts interior
addresses. The cost is accepted by the design and shrinks with count
elision (§23.25) and the planned removal of reference counts
(`POINTER_LOANS.md` phase 10, git history at 7ccbb4ea8; now [ownership.md](../compiler/ownership.md#references-count-nothing-formerly-phase-10)), so it is not raised here.

### 3.2 L4: §23.8 Disagrees With §23.19

§23.8 says a `dyn` value's data is "the value itself, or a box of it".
§23.19 and the code describe only reference forms, so there is no box
[V]. Delete "or a box of it".

## 4. Emitted Wasm

### 4.1 Method

One agent read all 483 tracked `.wat` baselines: 89,499 instructions,
not counting comments and headers. It counted patterns with scripts that
walk the instruction lines, and treated the `;; ...` elisions in 64 files
as barriers. I reran the excerpts of W1, W2 and W7 and the counts of
W1 and W6. `stage_fixtures_match` enforces the baselines, so they match
the current compiler.

The emitted code is clean at the small scale. Branches on constants,
empty blocks, `eqz; eqz`, `tee; drop` and unfolded constants do not
occur. Every gap below comes from a missing optimizer capability, not
from careless emission.

### 4.2 Findings, Ranked By Frequency

| # | Cause | Occurrences | Fix | Reference |
| --- | --- | --- | --- | --- |
| W1 | One scratch local per function copies every count operand | 963 `get; tee` in 188 files, 810 of them just before a count guard `if` [V] | reuse the operand's local when it already is one, or give each site its own local | TurboFan's SSA hides it, so this costs size and Liftoff speed only |
| W2 | 70% of count calls take the page-table path | 1,176 of 1,672 count calls, in 209 files [agent] | carry a "starts a block" fact from `rt:new`, `rt:str`, `rt:ctx_new` and frames, and emit `*_known` | Swift counts only the owning object |
| W3 | The suspending-frame protocol tests and updates the frame piece by piece | 493 `local.get $frame; if` in 42 files, and 246 back-to-back read-modify-writes of the flags word [agent] | merge same-condition `if`s and combine the flag stores, or clone the body with and without a frame | LLVM SimplifyCFG, InstCombine and DSE |
| W4 | No redundant-load elimination or store-to-load forwarding | 236 repeated loads with no store or call between, 40 store-then-reload pairs, 53 drop glues that load and test a field twice [agent] | a memory-aware local value numbering pass over linear memory | LLVM EarlyCSE and GVN |
| W5 | Null tests on values that cannot be null | at least 127 in 79 files, plus about 300 `if $frame` tests [agent] | add a known-nonzero fact to opt rule 12, seeded by allocator results | LLVM CorrelatedValuePropagation, `nonnull` |
| W6 | `string.len` stays a call, which forces its receiver into a frame slot | 56 calls in 31 files [V]; 115 copies of the 24-instruction frame prologue with `memory.grow` in 87 files [agent] | inline it, or promote it by value as `arg-promotion` does for user code; move the grow path into one shared helper | rustc inlines `#[inline]` std accessors across crates |
| W7 | Retain and release pairs that cancel, left after inlining | 10 back-to-back pairs in 8 files; up to 86 farther apart in 57 files [V for the excerpt] | pair counts again after inlining | Swift ARCSequenceOpts, LLVM ObjCARCOpts |
| W8 | One poll and cancel wrapper per instance | 90 `.poll` wrappers with 21 distinct shapes, 89 `.cancel` with 19 [agent] | one wrapper per result signature, reaching the body through the frame | Rust has none: the state machine is `poll` |

### 4.3 Excerpts

**W1.** `crates/hd_driver/tests/baselines/own/physical-reference-projections.wat:67`.
The emitter is `count_tests::lower`, which makes one local `t` per
function (`crates/hd_wasm/src/count_tests.rs:76`). That local has many
writes, so copy forwarding (opt rule 4) cannot fire. It conflicts with
every value live at any count site, so `coalesce` (rule 15) cannot merge
it.

```wat
local.get $ctx
local.tee $t0
if
  local.get $t0
  call $rt:release_ref'1
end
```

**W2.** `Em::release_ctx` (`crates/hd_wasm/src/emit/ctx.rs:253`) releases
a context that `push_ctx_of` just built through `Helper::ReleaseRef`.
That path does the null test and the `block_of` lookup.
`Helper::ReleaseKnown` is enough. More generally, `emit/count.rs` picks
the known form from the type's `Walk::known()` only, not from where the
value came from. The snapshots have 44 sites that release a fresh
allocation through the slow path.

**W3.** `crates/hd_driver/tests/baselines/drive/block-on-drives-a-known-call-at-once.wat:22-99`
is a body with no suspension point. It tests `if $frame` four times.
It loads, changes and stores `offset=12` four times: it saves and sets
RUNNING, sets EXITED, then restores RUNNING. Each piece comes from a
separate helper that guards itself (`task::guard_frame`,
`Em::exit_frame`, `flags_or`, `set_flags` in `emit/susp.rs`). The
optimizer has no if-merging and does not combine stores.

**W4.** `crates/hd_driver/tests/baselines/arg-promotion/a-reference-passed-on-or-compared-keeps-its-address.wat:89-115`
loads `p.x` and `p.y` three times each in one straight-line expression.
`opt.rs` rules 1 to 20 reason only about locals and constants.

**W6.** `crates/hd_driver/tests/baselines/features/return.wat:70-152`
keeps `r` in a linear-memory frame slot only to pass `&r` to
`impl string.len`. The body of `len` is one load. Why `hd_opt`'s inliner
(`INLINE_LIMIT` 16) skips it was not found.

**W7.** At `a45fdfc17`,
`crates/hd_driver/tests/baselines/features/return.wat:113-130` was the
inlined `ret_param(r.name)`. It retained the owned result and at once
released it, twice, though the case's `expected:` line says no count.
Commit `70b014d4d` ("Forward borrowed results through inline value
wrappers") has since removed these, and the file now has no `retain`.
The other pairs stand; `counts/recursive-closure-value-capture.wat` and
S2 below show more. Nothing that runs between a retain and its release
can start the collector without a call, so removing an adjacent pair is
always safe.

```wat
local.tee $t2
local.tee $t3
if
  local.get $t3
  call $retain
end
local.get $t2
local.tee $t3
if
  local.get $t3
  call $rt:release_ref
end
```

### 4.4 Checked And Fine

- **`call_indirect` through a constant index** (169 sites) is the cold
  call design (DESIGN.md "Cold calls", #165). It keeps V8 from inlining
  the grow, copy and panic paths.
- **Multiplies by 4, 8 or 16** (315 sites) are kept on purpose
  (`opt.rs:1839`). They are the same size as a shift, and V8 lowers them
  to one.
- **The out-of-line `rt:release_ref` after the inline header test** is
  measured and documented in `count_tests.rs`. The wasmtime pipeline
  inlines the decrement (`opt::inline_counts`).
- **Bounds-check panics** (383 `rt:panic` calls): the element-access
  suite pins which ones stay. None repeats within a straight-line run.

### 4.5 Proposed Tickets

AGENTS.md "Performance Work" asks for one ticket per cause, measured on
every workload the cause touches. In order of expected value:

1. W2 and W5: a value-origin fact ("starts a block", "nonzero") carried
   through emission. It turns slow count calls into known ones and
   removes null tests.
2. W4 with W3: local value numbering over linear memory, plus merging
   same-condition `if`s. One pass covers both.
3. W1: count-site locals. Mostly a size and baseline-tier win.
4. W7: count pairing after inlining.
5. W6 and W8: each local and small.

Each ticket states its run-time and code-size change on `bench/runtime`
and `bench/study`, not only on the snapshot that shows it.

## 5. The Other Snapshots

The repository tracks 1,062 other stage snapshots besides the `.wat`
files:

| Kind | Files | Shows | Reviewed |
| --- | --- | --- | --- |
| `.own` | 370 | the ownership passes' moves, borrows and drops | §5.3, by an agent |
| `.counts` | 351 | the reference-count operations that survive | §5.3, by an agent |
| `.tir` | 269: 233 in `baselines/`, 32 in `crates/hd_tir/tests/corpus/`, 4 in `crates/hd_driver/tests/tir/` | the typed IR after checking | §5.3, by an agent |
| `.tree` | 53 | the parser's tree | §5.2, by me |
| `.layout` | 13 | `views::inspect_layout` of declared types | §5.1, by me |
| `.tokens` | 6 | the lexer's tokens | §5.2, by me |

### 5.1 Layout Snapshots

All 13 files in `crates/hd_driver/tests/baselines/layout/` were read in
full [V]. Each row was compared with what rustc gives the same shape on
wasm32. They agree in every case checked:

| Shape | hd | rustc on wasm32 |
| --- | --- | --- |
| `i32?`, `i32??`, `i32???` | 8 each; the tag's spare values hold the extra `.None`s | 8 each |
| `i64?` | 16, align 8 | 16 |
| `bool?`, `bool??`, `bool???` | 1 each | 1 each |
| `char?` | 4; `.None` is `0x110000` | 4 |
| `string?` / `List[i32]?` | 8 / 4: the null address | same as `Option<&str>` / `Option<Box<_>>` |
| `string??` / `List[i32]??` | 12 / 8: the address has one spare value, so the second `?` is tagged | same |
| `Either[string, i32]` | 8: `i32` beside the null address | 8 |
| `Either[bool, bool]` | 2 | 2 |
| `{flag: bool, v: i32?, w: i64}` | 24: `w`, `v`, `flag` | 24 |
| `{a: i32, b: f64, c: bool}` | 16: `b`, `a`, `c` | 16 |
| 300-variant enum | 2-byte tag | `u16` tag |
| `std.http.Cookie` | 72, and `Cookie?` 72 through the first field's tag | 72 by the same rule |

One small gain over the current shapes is in std, not in layout:
`std.time.Duration?` is 16 bytes because `Duration` has no spare value.
Rust's `Duration` keeps its nanoseconds in a type whose range leaves
spare values, so `Option<Duration>` is free [I]. This is a std design
choice, not a layout defect.

No layout snapshot pins a type over 2 KiB or a list buffer's header. A `.layout` view of `List[T]`'s buffer
(header, owner word, element stride) would pin §23.7.

### 5.2 Lexer And Parser Snapshots

The 53 `.tree` and 6 `.tokens` files are shape checks, so nothing in
them bears on generated code. Two observations:

- **Recovery is pinned.** The error-recovery trees
  (`parser-error-recovery.tree`,
  `a-later-statement-in-the-same-body-still-reports.tree`) show `Error`
  nodes that end at the right token, with later statements still parsed
  [V].
- **Fifteen node kinds have no parser snapshot.** No `.tree` shows
  `ForExpr`, `WhileExpr`, `BreakStmt`, `ContinueStmt`, `TupleExpr`,
  `ElseIfClause`, `ComprehensionExpr`, `SuspendExpr`, `SpreadExpr`,
  `ExprStmt`, `DiscardStmt`, `UsingStmt`, `ContextExpr`, `ImplType` or
  `InferType` [V: the `SyntaxKind` list in `crates/hd_syntax/src/green.rs`
  compared with the node names in every `.tree`]. Conformance cases
  likely parse them [I]. A tree snapshot of each pins its shape, so a
  precedence or recovery change shows up in review.

### 5.3 Ownership, Count And TIR Snapshots

**Method.** One agent scanned every tracked file at `101151bf0` by
script: 371 `.own`, 352 `.counts` and 269 `.tir`. It parsed the
`> %N = ...` lines, the `summary kept [...]` lines, the `uses $k:` lines
and the count lines with their reasons, and read about 60 files by hand
across every topic folder. I reread the excerpts of S1b and S3.

**Correctness.** Nothing unsound was found: no use after a move or a
drop. One oddity:

- `summary kept` lists 29 `&T` parameters in 26 files, such as `self:
  &Dog` in `features/dyn-calls.own`. The metadata disagrees with the
  type, but no harm was found.

**Findings, ranked by frequency.**

| # | Cause | Occurrences | Source | Reference |
| --- | --- | --- | --- | --- |
| S1 | By-value parameters are judged owned ("kept") when the body only lends them | 331 kept parameters in 120 `.own` files; 132 bodies keep a counted parameter but return a scalar. "last use" is 938 of the 1,822 `.counts` lines | `hd_tir/src/own/derive.rs`, `solve` and `kept()` [V] | Lean 4 borrow inference; Swift `@guaranteed` with OSSA |
| S2 | Inlined bodies are not simplified before ownership: aggregates and options are built only to be projected or matched | `an-inlined-user-leaf-counts-as-its-call`, `generics/use_generic` (6 count operations where 0 do); range structs built and read back in 23 `.own` and 7 `.tir` files | no construction-then-projection or known-constructor fold before `hd_tir::own` [I] | GHC case-of-known-constructor; SIL `destructure_struct`; Perceus fusion after inlining |
| S3 | An exhaustive match tests its last variant, with an unreachable `never` block | 33 scrutinees in 24 `.tir` files, 60 in 42 `.own` files | `hd_check/src/pat.rs`, `switch` and `switch_from` [V] | rustc's one `SwitchInt` with `otherwise: unreachable` |
| S5 | The iterator protocol pays per element | `Iterator.next` stays a call in 5 `.own` files; `release T? (last use)` per step in 14 `.counts` files | `ListIter` takes the list by move, so the list parameter is kept | Rust yields `&T`; Swift inlines `next` and cancels the pair |
| S6 | Suspension frames retain and at once release; 37% of count lines say only "counted reference" | 108 adjacent pairs in 40 `.counts` files; 680 of 1,822 lines unlabelled | provider rows and children (`Retain Cold row`, `Release FrameDrop row`); part is #1355 by design | — |
| S7 | `.own` and `.counts` disagree by construction | in 204 cases with both: 782 `Retain` in `.own` against 402 in `.counts`; 1,303 `Release`/`Drop` against 915 | `Retain Entry`, `Release Exit` and operand retains are ABI adapters that may lower to nothing | — |
| S8 | `expected:` lines and `known-bad-tir` titles that the snapshot no longer meets | at least 16 snapshots; 7 of 18 `known-bad-tir` titles | — | — |
| S9 | Empty `Block`s and no-op `Coerce`s in TIR | 681 empty of 1,516 `Block`s in about 8,000 instructions; 8 no-op `Coerce`s | — | — |

**S1, three sub-causes.** Each costs a retain in the caller and a
`release X (last use)` in the callee.

- **S1a.** Taking `&param` makes the parameter kept and puts it in a
  frame slot (21 parameters in 13 snapshots). `lent` requires that no
  address of it is held, so an address returned, stored or turned into a
  `&dyn` fails even with no write (`derive.rs:1729`). In
  `features/values-lifetime-structs.own`, `length` shows
  `%1 = Ref local frame [$0]`, then `summary kept [$0]`, and its
  `.counts` shows `release string (last use)`.
- **S1b.** Copying one element out makes the whole list kept (5
  snapshots). `crates/hd_driver/tests/baselines/features/index.own:41`,
  `list_take(xs: List[string], i)`:

  ```text
  %2 = Read copy [$0 [%1]] : string
  %3 = Drop [$0] : ()
  summary kept [$0]
  ```

  Lending `xs` and retaining only the element is enough.
- **S1c.** `moves = needed && root_owned` prefers a move to a borrow. A
  parameter is moved into a temporary that is only projected, matched or
  lent, so it becomes kept (`traits-conversion` `name_len`,
  `option/question`, `branch-values/pick_len`). About 54 parameters in
  36 files, from a noisy tracer [I]. Multi-arm and join cases are #1365.

**S3.** `crates/hd_check/tests/baselines/checker/union-constructors-and-match.tir:79`
chains `SwitchTag %0 [#0 %4 %10]` into a second `SwitchTag` on `#1`
whose fail block is `Block - : never`. Whether the Wasm keeps the extra
compare was not checked.

**S8, the list.**

- Feature cases: `branch-values` (11 releases against "no count
  operation"), `values-lifetime-structs`, `values-dependent-results`,
  `expressions-index-traits`, `traits-conversion`,
  `traits-associated-types`, `generics`, `option`, `functions-receivers`,
  `functions-opaque-results`, `annotations-inline-cold`,
  `traits-erased-errors` and `expressions-equality`.
- Elsewhere: `hd_wasm` `unions/nested-active-payload` and
  `counts/an-inlined-user-leaf-counts-as-its-call`. The second has
  failed its `expected:` line since `a842017f2` removed the Wasm
  `count_pairs` step on 2026-11-07, as its commit message accepts.
- `known-bad-tir` titles that now show the wanted shape: the three
  field-write-rebuilds cases (now a direct `Store [$1 .x]`), the two
  reads-through-a-getter cases, `copying-a-struct…-uses-valuecopy`, and
  `an-element-method-call-unpins…`.

A snapshot whose `expected:` line is false says that a regression is
the intended output. These need either the fix or a new line.

**Checked and fine.** The `IsUnique` on `all: &mut List` in
`binding/bind_kept` (the target may be shared); the `IsUnique` after a
constant list literal (static data); the retain on a list passed while
`&xs[0]` is pinned (the pin needs it); the "cancelled" releases (unwind
paths).

**Tickets.** S1 is the largest count cost in the snapshots and sits
upstream of W2 and W7: a parameter that is lent needs no count at all.
It belongs first among the count tickets of §4.5, before W2. S2 is the
same pass as O3 in `optimization-audit-2026-11-13.md` (scalar
replacement before ownership). S3 and S8 are each small.

## 6. Generators

The scratch files do not persist. Each measurement used this program,
with `N` the element count and `C` the list count:

```hd
pub fn main() -> void $ Console:
    let mut all: List[List[i32]] = []
    let mut i = 0
    while i < C:
        let mut xs: List[i32] = []
        xs.reserve(N)
        let mut j: i32 = 0
        while j < N:
            xs.push(j)
            j = j + 1
        all.push(xs)
        i = i + 1
    println("${all.len()}")
```

Run with `/usr/bin/time -l hd FILE` for resident memory, or with
`/usr/bin/time -p hd FILE` for wall time.
