# TIR: Places And Explicit Ownership

Status: design record of TIR's places, read modes and ownership
statements, the ownership passes and the inliner, as built. The code and
the spec win. The design came from issue #667 (2026-10-21) and was built
in three stages; its migration plans, commit logs and measurements are
in git history (this file at 21fc0e06f). The owner chose Direction 3 of
the TIR v2 alternatives record (git history at 7ccbb4ea8).
The step plan that moves the rest of the backend's decisions into TIR is
[BACKEND_NEUTRAL_TIR.md](../future-work/BACKEND_NEUTRAL_TIR.md).

TIR itself, its columns, catalog and invariants are
[crates/hd_tir/DESIGN.md](../../crates/hd_tir/DESIGN.md) §4.13.11
([crates/hd_tir/src/ir.rs](../../crates/hd_tir/src/ir.rs)). The
ownership passes are `crates/hd_tir/src/own.rs` and `own/`; the inliner
is `crates/hd_opt/src/inline.rs`; the driver runs both
(`crates/hd_driver/src/own.rs`).

## Contents

1. [Summary](#summary)
2. [Decisions](#decisions)
3. [The IR](#the-ir)
4. [What the checker lowers](#what-the-checker-lowers)
5. [What the checker emits](#what-the-checker-emits)
6. [The ownership passes](#the-ownership-passes)
7. [The inliner](#the-inliner)

## Summary

TIR has one idea and one duty beyond a typed IR.

The idea is the **place**: a root (a local, a top-level binding, an
instruction's value, a pointer value, or a reserved frame slot) followed
by projections (dereference, field, list element, enum variant), as in
Rust's MIR. A place is an operand, not a value. Three instructions use
it: `Read` gives the value at a place, `Store` replaces it, and `Ref`
takes its address. A field write through `&mut Body` is one instruction,
`Store [*$0 .x] 1.0`, and a field read through a reference is one
`Read`. Struct field writes do not rebuild their parents.

The duty is **visible ownership**. Each `Read` says how it reads: `copy`
(takes a count), `move` (takes the place's count and leaves it empty)
or `borrow` (no count). Drops, uniqueness checks, retains, releases and
drop flags are statements. The checker writes none of this: it emits
plain reads (mode `use`). The ownership passes decide the rest, per
instance, after the folds and the inliner.

- **Steps.** [The pipeline source](../../crates/hd_tir/src/own.rs)
  lists the complete order. Cleanup scopes and cancel exits are lowered
  to blocks first (`own/scopes.rs`). Then come returned-loan places,
  local references, struct copies, temporaries, value aliases and
  forwarding, late reads, and insertion. Borrow, move, capture moves,
  known-null, hoisting and arm drops share one batch. Self-store
  narrowing and copied-parameter lending consume that result.
  Plain-reference ends precede overwritten-value releases,
  borrowed-result retains and operation and boundary counts, which share
  the final splice. Anchors and the non-`Copy` check finish the
  pipeline. Empty batches do not rebuild.
- **Only weakening holds up to the splice.** Insertion to hoisting only
  weaken, delete or move what insertion wrote. The steps after the
  splice add statements (`LoanEnd`, `Release`, `Retain`, boundary
  counts), so "correct after any prefix" covers the first part only.
- **Per instance.** The driver instantiates each instance from its
  item's checked body (`hd_mono::instantiate`: types substituted, bound
  calls chosen as impl or builtin calls), runs the structure folds and
  the inliner at its type arguments (`Env::inlined_instance`,
  `Env::fold_and_inline`), then the passes, so `OwnCx::counted` answers
  on concrete types. Dev and release builds run these stages in the same
  order. A generic local function is lifted to its own instance
  (`hd_tir/src/ir/lift.rs`), so no instance body holds a generic
  sub-body.
- **`OwnCx` has about 45 methods.** They include release summaries of
  callees, promoted arguments and exclusive copies, so the passes read
  facts of other bodies.
- **Cross-function and whole-program work.** The inliner copies callees
  into callers before the passes in every build (`hd_opt/src/inline.rs`).
  Release adds per-instance summaries with a fixed point over recursive
  groups, exclusive copies of bodies with reference parameters and
  argument promotion ([crates/hd_opt/DESIGN.md §9](../../crates/hd_opt/DESIGN.md#9-release-summaries)).
  Pointer-loan decisions are a whole-program link pass in every build
  (`hd_tir/src/own/pointer.rs`). The owner allowed whole-program and
  cross-function analysis on 2026-11-12 (docs/audit/review-2026-11-12.md
  §13, "Owner Answers").
- **Boundary ownership is recorded in TIR.** The driver's type-indexed
  `ParameterMode` query supplies the declaration and function-value
  conventions. After collection, `hd_tir::own::boundary::plan` records
  parameter and capture loans, consumers, and string-slice comparison
  loans per instance group. Emission reads those records; it does not
  repeat parameter-use, capture-use or trait-slot ownership analyses.
- **Counts lower through per-type shims.** A count of a value that is
  not one address calls its type's retain or release shim, a lowered TIR
  body per type (`hd_layout::shims`); a `Drop` hook runs in place, where
  the value lives.

## Decisions

**Fixed by the owner.** RC on wasm32 linear memory with the cycle
collector, never Wasm GC. A reference is one thin address. Interior
references carry no tag: a counted reference always refers to a whole
object (#912), so a `*T`'s counts read the header (crates/hd_wasm/DESIGN.md
§23.30, #988, #1444). Bounds-check elimination is separate (#711).
Compile time, memory, compiler size and cognitive load count as much as
run time.

**Decided by the owner (2026-10-21): the checker does not do reference
counting.** It emits places, plain reads, stores and references. All
count work (copies, drops, uniqueness checks, borrows, moves and
known-null) is done by passes over the same TIR in `hd_tir`. There is no
second IR, and the checker holds no count rule.
[Should the checker mark move, copy and borrow?](#should-the-checker-mark-move-copy-and-borrow)
settles the one open point: the checker marks no read mode at all.

## The IR

Most of TIR keeps its documentation in crates/hd_tir/DESIGN.md
§4.13.11: the flat columns of `Body`, values as instructions,
structured control, calls, closures, suspension, providers, coercions
and constructions. This section covers places and ownership.

### Places

A place names where a value lives. It is a **root** and zero or more
**projections**, read left to right (`ir::PlaceRoot`, `ir::Proj`).

| Root | Written | Meaning |
| --- | --- | --- |
| local | `$3` | a parameter, binding or hidden local of the body |
| global | `@app.main.counter` | a top-level binding, with its type |
| value | `%7` | an instruction's value: a temporary, read-only unless a dereference follows |
| pointer | `ptr %7` | the value a `std.rt.memory.Ptr[T]` addresses: a `Read` moves it out, a `Store` drops nothing (`module.rt.ptr.read`, `module.rt.ptr.write.over`) |
| frame slot | `^2` | reserved: a slot of the function's frame; no stage writes it |

| Projection | Written | Meaning |
| --- | --- | --- |
| dereference | `*` before the root or after a projection | the place a reference value refers to |
| field | `.x` | a struct field or tuple element, by declaration index; embedded fields are explicit steps |
| element | `[%5]` | a slot of a list's buffer at the index value `%5`, bounds-checked unless `hd_opt::prove` set the record word's `ELEM_UNCHECKED` bit (the text form does not show it); a map value's entry at an entry number (`MapEntry`) |
| variant | `as .Circle` | the payload of an enum value known to be that variant, before its fields |

Examples, with the source they come from:

| Source | Place |
| --- | --- |
| `body.x` where `body: &mut Body` | `[*$0 .x]` |
| `nested.body.x` on a local | `[$1 .body .x]` |
| `bodies[i].vx` where `bodies: &mut List[Body]` | `[*$0 [%5] .vx]` |
| `f().x` where `f` returns `&Point` | `[*%3 .x]` |
| the payload `r` of `.Circle(r)` matched in `$0` | `[$0 as .Circle .r]` |

Two places **overlap** when one's projection list is a prefix of the
other's after their roots match, ignoring element indices (two elements
of one list may be the same slot). Overlap is the one aliasing test the
passes use; a dereference of a reference that came from elsewhere
overlaps every place of its referent type that a reference can reach.
A reference reaches a local only when one outlives the one call it
lends to, or a closure shares the local: a local reference that one call
takes, and whose result holds none, is dead outside that call, so a
store into the local elsewhere overlaps no dereference.

A place's intermediate types are not stored. The instruction's `ty` is
the place's type; each step's type is the base type's declared field
type, substituted.

### Reads, stores and references

| Tag | Operands | Type | Meaning |
| --- | --- | --- | --- |
| `Read` | a: place, b: mode | the place's type | the value at the place. Mode `use` is the checker's; the passes replace it with `copy`, `move` or `borrow` |
| `Store` | a: place, b: value | `void` | replaces the value at the place: the new value moves in, and a `Release` after it drops the old one. A store known to find its place empty drops nothing (`own/overwrites.rs`) |
| `Ref` | a: place, b: `0` shared or `1` mutable | `&T` or `&mut T` | the address of the place. A local root makes the local addressable: a frame slot or a cell, as the storage pass decides |

**Read modes.** A value produced by `copy` or `move` owns one count of
each counted leaf it holds; a `borrow` value owns none and is valid
while its place is unchanged.

| Mode | Count | The place afterwards | Who writes it |
| --- | --- | --- | --- |
| `use` | undecided | unchanged | the checker |
| `copy` | one more of each counted leaf, stated as a `Retain` | unchanged | insertion |
| `move` | the place's own | empty: zeroed, or known dead | move |
| `borrow` | none | must not change while the value lives | borrow |

For a type with no counted leaf every mode costs nothing, and the
emitter emits the same loads for all four.

**Where values are owned.** A call's result, a construction, a `copy` or
`move` read, a `Ref` of a celled local and a `Closure` produce
owned values. A use of a value either **consumes** it (a `Store`, an
owned argument, a construction part, a `Return` or `Break` value, a
capture, a coercion that keeps its operand's values) or **lends** it (a
borrowed argument, an operand of `Prim`, an element index, the root of a
place). After the passes, every owned value has exactly one consumer or
one `Drop`. A coercion that keeps its operand's values (`Weaken`,
`Variance`, `RowSubsume`, `SuspendFnToCtor`, `Identity`,
`ProviderAsRef`) is its operand under another type: it takes the
operand's count, and a coercion of a `borrow` read is a borrow too.

### Ownership statements

The passes write these; the checker writes none but the pointer loans.

| Tag | Operands | Meaning |
| --- | --- | --- |
| `Drop` | a: place | releases the value at the place and leaves it empty. A drop of an empty place does nothing |
| `IsUnique` | a: place of a list or map | makes the collection's buffer the place's own: copies it when another holder shares it. Precedes every in-place change |
| `Retain`, `Release` | a: the instruction whose value they count, b: a shared count record | a count the passes state: a copy's retain, an overwritten value's release, a borrowed result's retain, an operation's or a boundary's count (`own/counts.rs`) |
| `MakeUnique` | a: an instruction, b: a count record | a deferred operation's uniqueness action |
| `FlagSet`, `FlagClear` | a: place, b: local | set and clear the drop flag local b of move path a, a part of a local or a value in place (`own/drop_flags.rs`, #1420). Every init of a path sets the flags at and under it, a move or a repeated drop clears them; a `Drop` hook runs when its nearest flag and every flag under it hold. A cell's or frame slot's statement writes the flag slots of its layout instead of b |
| `Box` | a: local, b: a box word | makes local a's cell where its scope starts (`own::storage::boxes`) |
| `LoanBegin`, `LoanClose` | a: the loan's site or `LoanBegin` | run-time pointer loans, which the checker places from its lifetime regions (`own/loans.rs`; docs/compiler/ownership.md) |
| `LoanEnd` | a: place | ends the pointer loans a whole value or local holds, with no release: the `Drop` of a value that holds only references that can own nothing (`own/plain.rs`) |
| `OwnLoans` | a: place of a counter local | a use of the own-loan counter a closure shares (`own::loans::share_counters`) |

**The exit set.** A body has one exit (`Return` branches to it). Its
`exit_drops` column lists the owned locals that may still hold a value
there; the exit drops them. Insertion fills it, and known-null removes
the locals that are empty on every path to the exit. A `Break` or
`Continue` that leaves a block gets explicit `Drop`s of what that block
still owns, before the jump. Each suspension point's cancel block
(`Body::cancel`) releases what the point holds, runs the cleanup suites
around it and returns cancelled.

### Encoding, wire format and caching

**The place word.** A place operand is one data word. With bit 31 set,
it is a bare local `$n` (bits 0 to 30), with no record, so a plain local
read costs one word. With bit 31 clear, it is the offset of a place
record in `extra` (`ir::place`):

```text
[len, root kind, root word, (root word 2), projection word ...]
  root kind 0 local     root word: LocalId
  root kind 1 global    root words: DefId, Ty         (ID words, remapped by the wire)
  root kind 2 value     root word: Ref
  root kind 3 frame     root word: slot               (reserved)
projection word: top 2 bits the kind
  00 field     bits 0..29: field index
  01 deref     no payload
  10 element   the next word is the index Ref
  11 variant   bits 0..29: variant declaration index
```

`Read` keeps its mode in data word b. `Point.shift`'s `self.x += dx`:

```text
local $0 self: &mut Point [param]   local $1 dx: i32 [param]
extra:  @0 [4, 0, 0, 1<<30, 0]       # $0, deref, field 0 (x)
%0 = Read      a=@0       b=use : i32
%1 = Read      a=$1|bit31 b=use : i32
%2 = Prim Add  [%0 %1]          : i32
%3 = Store     a=@0  b=%2       : ()
```

The read and the store share one record: a place record is immutable
data, and a place's root and indices are values evaluated once.

**Wire format.** `write_body` and `read_body` write per-body tables,
then the columns, with ID words of `extra` remapped by `id_words`: a
place record's global root has its `DefId` and `Ty` as ID words, every
other word is copied. Since instructions share place records, a body's
ID words are collected once per `extra` position before they are
remapped. The `check` entry stores the checker's TIR (modes `use`, no
ownership statements). An owned body's pass columns (`exit_drops`,
`cancel` and the rest) go in an owned section after the locations,
which only the dev build's stored `owned` entries carry
([where and when the passes run](#where-and-when-the-passes-run);
crates/hd_tir/DESIGN.md, "The wire form").

### Invariants

The verifier's invariants are numbered in crates/hd_tir/DESIGN.md
§4.13.11's list. The ones about places and ownership:

1. **Places are well formed.** A local root names a local of the body;
   a value root precedes its user and is visible there; an element
   index precedes its user; no stage writes a frame root.
2. **Stores and mutable references reach writable places.** A `Store`
   or a `Ref` with `b = 1` has a local, global or pointer root, or a
   dereference or an element in its projections. A value root is never
   written in place.
3. **Checker output has no ownership.** Before the passes every `Read`
   has mode `use` and there is no `Drop`, `IsUnique`, flag statement,
   `exit_drops`, `cancel` or stable-list row. The checker does write the
   run-time pointer loans, `LoanBegin` and `LoanClose`
   (`hd_tir::own::loans`), from its lifetime regions; the passes copy
   them as any statement, and the first one names a counter local and
   adds its `OwnLoans` uses where a closure shares a pointer's own loans
   (`own::loans::share_counters`).
4. **After the passes, ownership balances** (checked by `verify_owned`
   in debug and checked builds): every owned value has exactly one
   consumer or one `Drop` on each path; no `Read` of a place follows its
   `move` on any path without a `Store` in between; no `borrow` value is
   live across a write to an overlapping place or a suspension point.

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
drops, snapshots, uniqueness and reference forms belong to passes over
TIR or to the emitter. A checker change that adds representation work
is a design violation.

## What the checker emits

The checker's forms, as the `tir` stage view prints them. Notation: `$n`
a local, `%n` a value, `[...]` a place. Ownership appears only after the
passes (the `own` view).

### Locals and `let mut`

A `let` binding, `:=` and a `let mut` are all locals; `mut` matters only
to the checker's writability rules. `let i = n; i = i - 1` is
`Store [$1] (Read [$0])`, then `Store [$1] (Prim Sub [Read [$1] 1])`. A
local declared with no initializer is not stored until its first
`Store`, which finds its place empty and so releases nothing.

### Field reads

`p.x` on a local is `Read [$0 .x]`; through a reference,
`fn by_ref(p: &Point) -> i32: p.x` is one `Read [*$0 .x]`. The emitter
loads only `x` (crates/hd_wasm/DESIGN.md §23.31).

### Field writes

`nested.body.x = 1.0` on a local is `Store [$1 .body .x] 1.0:f64`: no
parent is rebuilt. Through `&mut self`, `self.x += dx` is
`Read [*$0 .x]`, `Read [$1]`, `Prim Add`, `Store [*$0 .x] %2`. A
top-level binding: `Store [@app.main.total .count] v`.

### Element reads and writes

`bodies[i].x = bodies[i].x + dt * bodies[i].vx` through
`bodies: &mut List[Body]` reads `Read [*$0 [%0] .x]` and
`Read [*$0 [%0] .vx]`, each loading one field, and writes
`Store [*$0 [%0] .x] %8`. Here `*$0 [%0]` means: the list `$0` refers
to, its buffer, slot `%0`. A store through an element projection makes
the buffer unique first: insertion puts an `IsUnique [*$0]` before it,
which hoisting can move out of a loop. A whole-element store `xs[i] = v`
is `Store [$0 [%1]] v`.

A map value is a place too (decision E, #494; `ref.elem.map`,
`types.coll.map-value.place`), with the key's entry in place of the
index: `m[k].x = v` is `%e = Intrinsic MapEntry [%m %k]` and
`Store [%m [%e] .x] v`, and `&mut m[k]` is `Ref mut [%m [%e]]`. A plain
`m[k]` read is `Read [%m [%e]]` too, and so is the receiver of
`m[k].push(v)`; `m[k] = v` is `MapSet`, since a new key adds an entry.

### `&` and `&mut`

`r := &mut x; *r = 5` on a `let mut x` is:

```text
%0 = Store [$0] 1:i32 : ()
%1 = Ref mut [$0] : &mut i32
%2 = Store [$3] %1 : ()
%3 = Read [$3] : &mut i32
%4 = Store [*%3] 5:i32 : ()
```

`&e` on a value that is no place makes a new object: the checker binds
the value to a hidden local and takes `Ref [$k]` of it. An element
reference `&xs[i]` is `Ref [*$0 [%5]]`, with the index value `%5` (an
element index is always a value, evaluated once); insertion adds the
`IsUnique`s before it ([insertion](#insertion)). `*r` as a whole value
is `Read [*$0]`; `*r = v` is `Store [*$0] v`.

Identity (`is`, `identity_of`) needs nothing in the IR: the backend
compares addresses (crates/hd_wasm/DESIGN.md §23.20).

### Calls and pass modes

The pass mode of an argument is its parameter's type, which the passes
read from the callee's signature. The checker emits the argument values
in order.

| Parameter | The checker passes | After the passes |
| --- | --- | --- |
| `p: T` (an owned copy) | `Read [place]` or any value | a `copy` read, or `move` at the place's last use; a temporary moves in |
| `mut p: T` (the callee's changeable copy) | the same | the same; the callee owns and may write its local |
| `p: &T` | `Ref [place]`, or a reference value | the reference is lent: neither side counts it |
| `p: &mut T` | `Ref mut [place]` | lent; the place must be writable (the checker's rule) |
| `&self`, `&mut self` method on a place | `Ref [place]` (auto-address) | lent |
| `&self` method on a temporary | a hidden local and `Ref [$k]` (a new object) | lent; the hidden local is dropped after the call |

A bound method reference `counter::bump` is a `Closure` over a hidden
local holding the receiver.

### Closures and captures

A closure is a sub-body in the same columns; its captures and their
modes (`Copy`, `Move`, `Shared`) are the capture columns, and reads
inside the sub-body name the outer local. A `Shared` capture lives in a
cell, as an addressed local does. The passes run per sub-body, with the
captures as locals that hold a value on entry.

### Match and patterns

The decision tree reads payloads as places: `.Circle(r) => r * r` on
`s: Shape` is `%8 = Read [$0 as .Circle .r]` and `Store [$1] %8`.
`Unwrap` of an optional is `Read [$0 as .Some .0]`. A match on an owned
temporary (`match parse(text):`) roots its payload reads at the
temporary, `[%3 as .Ok .0]`, which is what lets the move pass move one
payload out and drop only the rest (#679).

### `for` and `while`

`while` is a `Loop` whose body is `If c then body else Break`. The
checker expands a counted `for` into ordinary control flow
(crates/hd_mono/DESIGN.md §12.5): a range keeps a cursor, end and
inclusive flag; a list or map loop reads indexed elements or live
entries, holding its source value (`flow.for.snapshot`); anything else
calls `iter` and `next` and matches the optional item.

### `defer` and `using`

A cleanup `Scope` lists its suites, and a `Defer` registers one.
`using x = e` is a `Scope` whose suite calls `dispose` (or `dispose!`)
on the binding; the checker's `using.rs` takes a reference to the
binding, so it gets `Ref` and `Read` like every other site. The first
ownership pass lowers each cleanup scope to blocks
(`hd_tir/src/own/scopes.rs`): the scope's end runs a copy of each suite
registered so far, last in, first out, and each way out breaks to a
chain that runs them before the jump. The later passes see those copies
as ordinary code.

### Suspension

A bang call is `Await`, `s!()` is `AwaitValue`, `join!` is cold `Call`s
then `AwaitAll`, `race!` a `NewList` of cold suspensions then
`AwaitRace`. The passes treat a suspension point as a barrier: no
`borrow` value is live across one, since another task may change the
place while this one waits, and each point has a cancel block.

### Providers

`With`, `ProviderGet`, `ContextNew`, `ContextFor`. A provider of a key
whose trait has a `&self` method is a trait value reference;
`$.use(Mailer)` is `ProviderGet` then `Coerce ProviderAsRef`. A method
call through it reads the trait value through the reference,
`Read [*$1]`. A `With` block lends its providers.

### String interpolation

`"hi $name #$n"` is ordinary calls of `StringBuilder`'s private
helpers (`lib/std/text.hd`): each piece's width is measured first, then
one `value_new` allocates the result and `value_text`, `value_signed`,
`value_unsigned` or `value_char` writes each piece at its offset, in
source order (`expr.interp.builder.sized`, `expr.interp.write.measured`;
`Ck::string_expr`). A constant interpolation is a string constant. There
is no `Interp` instruction.

## The ownership passes

`hd_tir::own` runs the passes over one body in the order
[the summary](#summary) gives. Each pass reads the body and rewrites
read modes or adds and removes ownership statements; none changes what
the program computes. The core is insertion, borrow, move, known-null
and hoisting, described below; before insertion, late operand reads
reorder a few pure reads ([Late operand reads](#late-operand-reads)).

### Where and when the passes run

The passes run once per instance, on its substituted, folded and
inlined body (`hd_driver/src/own.rs`), on every body the program
reaches: a decoded body, and one collection generates (a supplied or a
`dbg` body, a function reference's call body). A std body's owned form
in a dev build is the same in every program, so the driver stores it
and later builds decode it instead of running the passes again
(`Run::owned_std_key`, #1423). A package body's owned form is stored
too, keyed by the body, the program's declarations and the build's
settings, with the bodies the folds and the inliner read, each by TIR
hash; a later dev build takes it while each still hashes the same
(`Run::package_owned_key`, #2028). A generic instance's owned form is
stored the same way, keyed by its instance key and its generic body's
TIR hash (a compiler-supplied member's by its family's hash), with the
bodies its instance's folded and inlined form read
(`Run::instance_owned_key`, #2106). A release build stores none, since
its passes read callees' summaries. After a body edit, the passes run on
the edited bodies, the bodies that read them, and the instances of
either.

Dev and release run the passes from one driver, `Run::owned_bottom_up`:
the first time collection asks for a body, the walk owns it and what it
calls, callees' strongly connected components first, one level of
components at a time on the build's threads. Release records each
body's summary for its callers. Dev records none, so its components
make one level (argument promotion, which reads a callee's owned body,
keeps the levels), and it takes each node
the walk meets from the store when it is current, without inlining it,
owning it or walking its callees.

- **Why after decoding, not in `ModuleFinish`.** At decode the program's
  layout facts are at hand (whether a type has counted leaves), and the
  passes run only on the bodies a program reaches: 231 of json's 2,335
  checked bodies when this was decided. In `ModuleFinish` they would run
  on every body of every checked module.
- **Fixtures.** The `tir` view shows checker output, and the `own` view
  (`hd_testkit`'s stage views) shows a body after the passes, so every
  pass's decisions are reviewable as baseline diffs.

### What the passes read

The passes read the body and an interface the driver implements from
the program's tables (`hd_tir::own::OwnCx`). Its core, abridged:

```rust
pub trait OwnCx {
    /// Whether a value of `ty` holds a counted leaf.
    fn counted(&self, ty: Ty) -> bool;
    /// How argument `k` of `n` of a call of `callee` is passed: owned or
    /// lent.
    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass;
    /// Whether a call of `callee` may reach memory outside its
    /// arguments: false only for the few std functions known pure.
    fn may_write(&self, callee: &Callee) -> bool;
    // ...
}
```

These were designed as signature and layout facts, never another
body's. `OwnCx` has about 45 methods, and in release they include callee
summaries ([the summary](#summary)).

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
  rebuilt at a batch's end with its statements spliced in
  (`own/splice.rs`). Instructions are never inserted one at a time.

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
  drops; giving each its mode there is one more word write, and no
  checker code has to know where a collection must be copied.
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

A `!Copy` value is the one case decided by type: the checker reports a
read after its move as a source error (`ref.move.holds`), so the passes
never see a second read of a moved non-`Copy` place, and the last step
checks that no non-`Copy` read is a `copy`.

### Late operand reads

A call's or a construction's operand that reads a local's place, or
takes a reference to it, moves to just before its consumer when a later
operand reads an overlapping place of the same local and nothing in
between can change the local. In `m.at(m.infos[i].position)` the
receiver read is then the local's last access, so move takes it, or a
reference takes the local's own count (`Ref take`), as in
`p := m.infos[i].position; m.at(p)`. Read first, it would copy `m`.

Nothing between may store to the local, take a reference to it,
capture it other than by copy, change a collection, or suspend; when a
reference or a shared capture elsewhere may reach the local, no call or
store may stand between either. The place is the local or a field path
of it: an element's index check may panic, and a dereference reads
memory a call may change. A read has no effect, so the program computes
the same values. The pass rebuilds a body only when it moves a read.

### Insertion

Insertion is the naive, correct ownership of crates/hd_wasm/DESIGN.md
§23.16 plus last-use release, made explicit.

1. **Modes.** Every `use` read becomes `copy`. For a type with no
   counted leaf, `copy` is free; the later passes weaken the rest.
2. **Temporaries.** An owned value used by more than one consumer gets
   a `copy` for each extra consumer (`Read copy [%v]`), so each owned
   value has one consumer. An owned value with no consumer gets a
   `Drop [%v]` after the statement of its own block that holds its
   last use.
3. **Locals.** An owned local gets a `Drop [$l]` after the statement of
   the innermost block holding all its accesses that holds the last one.
   A frame local's accesses include each use of a value that may point
   into it (#1006,
   `crates/hd_wasm/DESIGN.md` §23.38), and so do a cell local's
   (one a closure captures), since a reference into what the cell holds,
   as a `UniqList` element's, takes no count of the cell (#1299,
   `hd_tir::own::frame` rule 4). Parameters and captures live
   in the sub-body's root block. A `Break` or `Continue` that leaves that block
   before the drop gets a `Drop` before it; the function's exit drops
   the rest through `exit_drops`.
4. **Cleanup suites.** `own::scopes` has already copied each suite to
   every way out of its scope, so a suite's reads are ordinary uses
   there.
5. **Uniqueness.** Every in-place change of a collection place gets an
   `IsUnique` of that place just before it: a `Store` through an element
   projection, `ListPush`, a read of an element that a change then goes
   through (`xs[i].push(v)`), `MapSet`, `MapRemove`, and a write through
   a map value. The intrinsics do not check for themselves.
6. **Element references.** A `Ref` of an element place gets `IsUnique`
   of its list places before it, outermost first, again after any index
   evaluation since its base was read. The reference counts nothing; the
   list's holder lives until its last use (#1336 phase 10).
7. **Collection copies.** A copy of a list or a map is a count of its
   buffer, which the backend takes as any address's count; the passes
   keep a parameter that a change reaches in place owned
   ([List and Map buffers](#list-and-map-buffers)).

Why it is sound: every owned value and local is dropped exactly once on
every path. On the normal path, by the drop after the last use; on a
jump out of the block, by the drop before the jump; at the exit, by the
exit set, where a place already moved or dropped is zero and drops as
nothing. That is the invariant the emitter relies on
(crates/hd_wasm/DESIGN.md §23.16 "One exit", §23.20 "Releases at last
use"), visible.

### Borrow

Borrow turns `copy` reads into `borrow` reads where the value can live
on the place's count. A read borrows when all of these hold over its
**span**, the statements from the read to its value's last use:

1. **Every use lends.** No consumer needs a count of its own (an owned
   argument, a store, a return). A borrowed value has no count to give.
   A coercion that keeps the value's values passes the read on: the
   coercion's uses must lend, and the span runs to its last use.
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
its value, and rule 1 ensures nothing keeps the value past the span.

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
   at the base's last use, becomes `move`, and the base's `Drop` stays
   whole and releases what the move left, the moved part being zero. A part
   whose type no instance copies is live per field: its read moves at
   that part's last use, while other parts of the local are still read
   (`ref.move.field`). A reference into the local keeps each field it may
   reach live until its last use. A field move from a cell empties that
   field; the cell's whole drop remains to release the others.
4. **References at last access.** A `Ref` of a celled local `$l` at its
   last access takes `$l`'s count of the cell, and `$l`'s drop goes
   (#701). A frame local's never does (#1006). A frame local's last
   whole read moves its slot's value out when the local's drop follows
   it, or when no use of what may point into the slot follows it on any
   path, drops aside: a second walk counts those uses as reads.

Why it is sound: the place is read nowhere after the move, so nothing
can see it empty; a jump past the move still drops the place, which
holds its value there, because the drop before a jump and the exit set
stay.

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
   no zeroing.

`own/known.rs` builds rules 1 and 3. Rule 2 is `own/overwrites.rs`: a
store known to find its place empty gets no `Release`. Rule 4 is not
built.

A celled local needs no rule: its `Box`, right before the statement that
declares it in the block holding every use (`own::storage::boxes`), makes
the cell on every path that reaches a use, so every use reads the cell
with no null test (#678, #2172).

Why it is sound: each rule removes work only on a place whose state is
certain on every path; an unknown state keeps the drop, the zeroing or
the test.

### Hoisting

Hoisting moves an `IsUnique` out of a loop (#653, #710). An `IsUnique`
of place `p` inside a loop moves to just before the loop, guarded by the
loop's entry test so an empty loop copies nothing, when:

1. the `IsUnique` runs on every iteration that is entered (no `If` or
   `Match` around it skips it), or whenever an inner loop is entered that
   stands so in the loop, at any depth; a shared buffer then costs a copy
   the loop did not need only if no iteration enters that inner loop. A
   loop with no entry test (`while true`) runs its body whenever it is
   entered, so its check runs before it unguarded;
2. the loop never copies `p` or an overlapping place, takes no `Ref` of
   it, and holds no suspension point. A local reference to
   one element of `p` that the loop uses up is an address that counts
   nothing, so it is allowed; a shared one only when a change's own check
   hoists `p`, since short loans already drop a check that serves a shared
   reference alone;
3. no call in the loop may write, unless `p`'s root is a local that no
   closure captures and no reference reaches while the loop runs, or the body is an exclusive copy and `p`
   is behind a reference parameter the loop hands to nothing: its callers
   leave nothing else able to reach `p` during the call.

A `break`, `continue` or `return` before a check can skip it. Such a
check is deferred to the first reached write instead of running before
the loop. A flag initialized at loop entry records that the buffer has
been made unique; later covered writes use that decision without another
uniqueness operation. The same non-sharing proof (rules 2 and 3) keeps
that decision valid. A loop that exits before its first write copies
nothing. An eager check remains before the loop when no exit can skip it.

A deferred decision does not mark its place stable from loop entry:
the first write can replace the buffer. When no operation in the loop can
grow or replace the header, the decision also caches the successful buffer.
The backend keeps the initialized bit in an unused alignment bit of that
private Wasm local; header zero has initialized encoding one. The header
is untagged before use. Growing loops keep a boolean decision and reload
the current buffer. An adjacent element store reuses the header its
uniqueness check just resolved, preserving its ordinary bounds check.

The same proof otherwise marks the loop's **stable list places**: a list place
whose header nothing in the loop replaces. The pass records them in a
side table, and the emitter keeps their buffer address and length in
Wasm locals for the loop (#710). A list place whose length nothing in the loop changes,
though an `IsUnique` in it may copy the buffer, gets a row that keeps
only the length. So does the whole of a local no reference, capture or
kept read reaches, across calls that may write, which rule 3 refuses for
hoisting: a caller lends a by-value parameter, and a binding local
borrows, only where no call may change the source. Each row also bounds the list's region around the
loop: the last instruction before the loop and the first after it that
may change the list, found by one forward and one backward sweep over
the body with the same per-instruction rule, so a length read past the
first bound keeps its length through any statement around the loop that
ends before the second (the counter proofs' question, #2175).

Why it is sound: a buffer made unique before the loop stays unique,
since nothing in the loop can make a second owner (rule 2) and nothing
outside can reach the place while the loop runs (rule 3).

In one block, a direct call receiving only scalar values and local
mutable element references preserves the uniqueness of a private collection
local. Its root is neither captured nor addressed as a whole. Making
the buffer unique separates earlier owners, and a reference into an
element cannot give the call another owner of the header. The same
block scan records that the element reference checked its index.
Copies, header or index stores, unknown calls, and control-flow
boundaries discard the facts. An element-reference drop ends its loan
without changing the header. A shared short loan can omit its preceding
uniqueness check, so it discards these facts.

### Why this order

Insertion makes every count explicit. Borrow runs next, because each
copy it removes is one that move need not look at. Known-null needs the
final moves and drops, so it runs after them. Hoisting runs on the
`IsUnique`s that survive. Up to the final splice every pass after
insertion only weakens or deletes, so stopping after any prefix of that
part still gives a correct program; the steps after the splice add
statements ([the summary](#summary)).

### List and Map buffers

A list or a map value is the address of its counted buffer (a map's
table), whose count is its holder count (crates/hd_wasm/DESIGN.md
§23.7, §23.17, §23.18). A copy of a collection is one more count of its
buffer (`rt:retain_known`), and a change first makes the buffer the place's
own: insertion puts an `IsUnique` before it, which copies a shared
buffer, and every change that may replace the buffer (an `IsUnique`
that copies it, a `push` that grows it, a put that grows a table,
`list_truncate`) names the place it changes, so the new address is
stored there. No pass decides a collection copy of its own: insertion's
copy read of a value with a collection leaf is that copy. So a match
binding of a map's list keeps its value when the map's list changes in
place afterwards: its count makes the buffer shared, and the change
copies it first. An element reference counts nothing; the passes keep
the list's holder alive while one lives, and the checker forbids
changing the list then (`ref.elem.access.*`).

## The inliner

Much of std's collection and iterator code is hd: an element access of
a `UniqList` is a call of `index`, and a loop step through an iterator a
call of `next`. Without inlining, each is a call in the loop. The
inliner removes that cost.

**What it does.** Per instance, before the ownership passes, it replaces
a `Call` whose callee qualifies (the rules below) by a copy of that
body: its
locals renumbered into the caller's, its labels and sub-bodies offset,
its parameters bound with `Store`s of the arguments, each `Return` a
`Break` to a `Block` around the copy, its type parameters substituted
by the call's type arguments, which may themselves be the caller's
parameters, so a generic caller stays generic. A trait-method call
inlines only when its choice is an impl (`ChoiceKind::Impl`); a bound
or a trait value is resolved per instance and stays a call. A callee
that suspends, calls itself or is an intrinsic stays a call
(`InlineEnv::inline_callee`). The inliner names each callee by one
`Key`, as rustc's `Instance`: an item at its type arguments, or a lifted
generic local function's instance. The ownership passes then run on the
combined body, so a `ListIter.next` inlined into a loop borrows its list
like a hand-written index loop.

Small requirement-row bodies use the same structural copy (#1394).
`With` remains a lexical scope and its provider operands are relocated;
`ProviderGet` carries its relocated key record into that scope. Known item
references become direct calls using the caller's providers. Known literals
can copy their provider lookups too.

This exposes the existing direct-provider ABI without a context allocation
or lookup.

A call of a generic local function is an ordinary call site too (#2222).
Its value is the `Closure` that instantiates the lifted function (Swift's
`function_ref`, the call its `apply`), directly or through a local stored
once with it, as an item reference's is. The inliner keys the copy by the
lifted instance, the item at the function's arguments
(`hd_mono::lifted_args`), copies the function from that instance's
template with its captures as its first locals, and binds them to the
locals the instantiation captured, as a literal's copy does. The rules
below decide it as an undecorated function's call; an instantiation that
only copied calls used goes. A value that reaches the call through a
local captures nothing, since the local may outlive what it captures.

It adds no row-specialized function entry and changes no callable
ABI. Context construction, suspension and cleanup suites keep their call
boundaries. Escaping reusable contexts, closures and cold frames still own
their providers through existing machinery.

Row-bearing functions remain ineligible for the quiet-constant shortcut: their hidden providers are
inputs even when they have no ordinary arguments.

Automatic copies share
the literal consumer metadata budget, checked before decoding and again
after nested copies; wide provider records cannot multiply by call sites.
A folded constant keeps only its formal parameters and result metadata;
dead tables from its source cannot accumulate through enclosing copies and
prevent subsequent folding. The existing size and four-round budgets also
apply. Explicit hints keep
their specified copy limits. This follows the
same structural integration used by [rustc's MIR inliner](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_mir_transform/inline/index.html).

One selection rule covers std, `std.rt` included (2026-11-02, #1228: no
runtime exception, and no inline hints in `lib/std/rt/`), and user callees
(2026-10-26, #856): a
non-recursive callee fitting the size budget of 16 (`INLINE_LIMIT`). A body
fits by its raw row count or by a lowering estimate (#1358). The estimate
charges executable rows, labelled blocks and the inlined root's return
scope. Plain suite wrappers cost nothing; call arguments and a returned
value add binding costs. Reads, stores and branches retain their costs.
The estimate does not newly admit reference-carrying results: those keep
the existing raw-budget call boundary and its loan-end contract.
Reference getters and proved argument promotion also qualify. In release,
a small reader may keep shared references to unpromotable referents or
Drop owners when it only reads through them and has no reachable write
effect (#1346). A callee that takes `&mut` keeps its call boundary, with
one exception: at a call in a loop, in release, a callee up to the `@inline`
budget (`AUTO_INLINE_LIMIT`) that returns nothing and whose copy only moves
scalars through plain memory, calling nothing but what never returns and the
compiler's own code.
Such a copy can reach no alias, so its binding's loan ending at the last use
and not at the return changes nothing. The fallback
requires each reference parameter to be read through; an unused receiver
keeps its argument loan. Other
promotable referents keep their existing call-site checks. The outer
reference must be known; its generic referent need not have a concrete
layout. The copy substitutes the already evaluated reference argument
into its read-through places, without an intermediate reference local or
an owner copy. Those read-through uses
also keep the existing non-escaping proof of the original call. Normal
ownership lowering preserves the reference's lifetime.

The limit applies to a callee with its own calls copied, and a callee
over it with them is copied as written. In a release build the caller
then judges each call that copy brought and that stands in a loop as its
own, for up to four rounds, so `xs[i]` on a `UniqList` in a loop copies
`index`, then its `len` and `Buffer.at`. Those two calls per element took
matmul from 25 to 64 ms on wasmtime and from 70 to 180 ms on V8. Outside
a loop the rounds saved little and cost binary_trees 10% on V8's default
tier: copies in the allocator's helpers changed what V8 inlines itself.

A copy binds a reference parameter whose argument is a fresh local `Ref`
at the parameter's local reference type, as `let r: &mut T = &mut x` does.
The binding holds the loans of the `Ref`'s place, and a read through it runs
no loan check when that place crosses no object, as in a `&mut` of a frame
slot. A counted carrier would check the slot's zero count word on each read
and hold no loan on what the place does cross.

The build profile also selects which ordinary bodies are candidates:
a dev build copies std callees and user functions marked
`@inline(.Auto)` or `@inline(.Always)` only, so other user code stays as
written for stepping, stack traces and `dbg`; a release build copies
every user function the rules admit. The budget was originally
selected by counting raw rows: 16 let spectral_norm's `a(i, j)` (15 rows)
be no call in a hot loop. The operation measure keeps that numeric budget
while admitting small matches without discounting call-boundary work. A function marked
`@inline(.Never)` or `@cold` is copied nowhere
([`annot.inline.never`](../../spec/lang/14-annotations.md#r-annot.inline.never)),
and each direct call of a `@cold` function goes through the function
table, so V8 does not copy it either
([`annot.inline.cold`](../../spec/lang/14-annotations.md#r-annot.inline.cold)).

The rule was tuned on the 18 programs of bench/study and
bench/runtime/progs against JavaScript, on three V8 tiers (default,
`--no-liftoff`, `--liftoff-only`), with Wasm bytes and json's release
compile time. Limits of 8, 12, 16 and 20, each with reference
parameters refused or allowed and with leaves required or not:

- **Reference parameters** (2026-11-06, #1399 28c, on all 32 programs and
  the #1291 workloads, V8 and wasmtime). The rule above took setter-shaped
  matmul from 41 to 28 ms and sieve from 73 to 51 ms on wasmtime, for 0.5%
  more Wasm. Each limit costs something when dropped: everywhere rather than
  in loops, 13 KB and no time; a callee that goes on calling, `Filter.next`'s
  copy of `Map.next`, closures +29% and iter-pipeline +12% on V8; a callee
  that touches a collection or counted value, route-search +3,036,300 count
  operations, since the passes' rules for a `&mut` parameter do not cover a
  copy's local that holds it; a callee that returns a value, such as a byte
  cursor's `next`, nested-display +12.7% on V8's default tier, where `main`'s
  loop stays in Liftoff, and dyn_iter -9%.
- **Reference parameters** (the first tuning). Allowing them in a callee that calls on
  copied `StringBuilder.push` (`&mut self`, which calls on) into
  a loop: string_build 92 to 130 ms and map_count 121 to 138 ms on the
  default tier, the same on the other two. Refusing them costs json
  about 4 KB (+3.8%), as `&self` methods such as `Field.default` that
  were copied are now functions; its run time moves within noise.
- **Leaves.** Requiring a leaf as well changes no run time beyond noise
  and adds bytes where a small wrapper of a call stays a call
  (fannkuch_redux +31 B, nbody +33 B). Allowing reference parameters
  in leaves only copies more of json's cold paths, such as
  `Parser.unexpected` (+2.6 KB), for no run time.
- **Limit.** At 12, spectral_norm's `a(i, j)` stays a call: 262 to
  543 ms with `--liftoff-only`. 20 adds bytes (nbody +90 B) and no run
  time. json's release compile time is the same at every limit
  (0.33 to 0.34 s CPU, within noise).

Against the earlier rules (std at 8 with references, user leaves at 16
without), the geomean of hd over JS goes from 1.212 to 1.182 on the
default tier, 1.128 to 1.105 with `--no-liftoff` and 1.616 to 1.566
with `--liftoff-only`; hello's Wasm does not change.

A release build inlines bottom-up (2026-10-26): before a call's callee
is measured, its own calls are copied into it as far as their callees
allow, at the call's types, and the limit applies to the result. A
function that only wraps a call of a leaf is then a leaf too and is
copied in turn; a generic function and an impl method count at the
instance the call names, so `apply[Double]`, whose body is
`s.step(x)`, becomes `Double.step`'s arithmetic, and a loop over
`apply(double, x)` calls nothing (bench/runtime/progs/trait-dispatch).
A function on the way down is never copied into itself or into a callee
it reaches, and a function that calls itself, or that the walk finds on
a cycle of calls, is copied nowhere, so recursion stays a call. A callee that outgrows the
limit this way is copied as written, when that fits. Each instance is made
once per run. A copy inside a copy keeps its frame: its `Body::inlined`
row is called at an anchor in the outer copy's row, so a panic reports
each inlined frame.

An explicit hint is measured on the declaration as written (2026-11-02,
#1235). Once that declaration qualifies, explicit hints inside its copy
are honored even when their copies grow the outer copy past `@inline`'s
automatic limit. `@inline(.Always)` remains unlimited.

A copy's parameter bound from a binding local borrows it the way any
binding local does, and does not stop that local from borrowing in turn
(2026-10-24, #667): in `for part in parts: n += part.len()`, the copied
`len`'s `self` borrows `part` and `part` borrows the element, with no
count per element. It is the borrow pass's local rule for a binding local
read into another; the copy is not treated differently.

The copy's `Store`s and its root's `Break`s join the caller's values to
the callee's, so both sides must agree on types where a call lets them
differ (2026-10-26, #851). A call of a function returning `impl Trait`
has the opaque result type, while the copy, substituted by the call's
type arguments, has the hidden one; so once a copy goes in, the caller
opens its own opaque results too (`hd_mono::reveal`), as an instance
would. A `mut self` parameter of a type with no changeable copy of its
own (a primitive, an enum, a collection) is a writable `T` in the callee, where
the call passes a `T`; the copy's local takes the argument's type. A
copy whose parameters or result still differ from the call's, or whose
types do not open, stays a call.

**What it costs.**

- **Caching.** The inlined body is part of the caller's code, so a
  caller's owned body depends on its callees' TIR hashes: a dev build's
  stored owned body records the bodies the folds and the inliner read,
  each by TIR hash ([where and when the passes run](#where-and-when-the-passes-run)).
- **Rule.** Inlining reads another body. It is cross-function work,
  which the owner allowed on 2026-11-12 (docs/audit/review-2026-11-12.md
  §13, "Owner Answers").
