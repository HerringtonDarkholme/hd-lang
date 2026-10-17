# Ownership And Borrows: Prototype Plan

This is the plan for the first implementation of
[Ownership And Borrows](OWNERSHIP_DESIGN.md) (issue #163). The design is
decided, including the six points the owner answered on 2026-10-17
(design section 6). The owner's order was "options study first, then
prototype"; this record plans the prototype and writes no code. Nothing
here is accepted language behavior, and the prototype changes no spec
text until the owner orders the ownership spec pass.

- [1. What The Prototype Proves](#1-what-the-prototype-proves)
- [2. Where It Lives](#2-where-it-lives)
- [3. Slices](#3-slices)
- [4. Hard Cases By Slice](#4-hard-cases-by-slice)
- [5. Questions For The Owner](#5-questions-for-the-owner)

**Decided by the owner, 2026-10-17** (these override the plan below where they differ):
- No opt-in switch: "just turn it on, no one is using this except me". Ownership checking is on for every program; programs without marks are unaffected because nothing in them is owned or borrowed. Drop the `HD_PROTOTYPE` variable, the toolchain-key option and the `ownership-prototype-off` code from every slice.
- Shared TIR and type pool for every program: yes (the wire format version changes once).
- The parser accepts the marks in every program: yes.
- The ownership pass reads TIR for bodies with marks: accepted.
- Fixtures go into `spec/conformance/` now, not `crates/hd_driver/tests/ownership/`. So each slice also writes the spec rules its fixtures cite (the owner's standing order: every decided behaviour ships with its spec rules and fixtures); the ownership spec grows slice by slice.
- `OwnOnly`, `DropOwn` and `drop` land in a new std module every program can see.
- Freshness also covers a later store into a plain field of an owned value (review case O10), not only construction.

## 1. What The Prototype Proves

Six adversarial reviews tested the design on paper, and the sixth closed
with "the model is increasingly ready for a checker prototype". Paper
cannot show three things: that a checker can decide the rules in near
linear time, that drops land exactly once in generated code, and that
programs without marks keep their code byte for byte. The prototype is
the smallest end-to-end slice that shows all three.

### In Scope

- **Syntax.** `own T`, `^T`, `^mut T` and `^{a, b} T` at use sites;
  `let own`; the receivers `own self`, `^self` and `^mut self`; lifetime
  parameters `data Parser[^doc]` and arguments `Parser[^{d}]` or
  `Parser[^]`; the bound `T < own Any`; `own fn(...)` (design 2, 4.1, 4.4).
- **Owned values and moves.** The mode rules, the freshness rule for
  `let own`, use-after-move, re-initialization by assignment, no
  partial moves, consuming patterns and match guards (design 4.1, 4.2).
- **Borrows of locals.** Loans made by `^` parameters and `^` locals,
  conflicts under non-lexical lifetimes, two-phase receivers, lifetimes
  named by variables, elision, lifetime parameters on data, and the drop
  check (design 4.3, 4.4).
- **Drop.** `DropOwn` runs at scope end, on `return`, `break`,
  `continue` and `?`, and on overwrite, with a drop flag where a value
  moves on some paths only; owned temporaries drop at the end of their
  statement; `drop(x)` (design 4.5).
- **`OwnOnly` checking.** Plain positions of an `OwnOnly` type, `own`
  and `^mut` fields, and ownership by structure for user data, `Option`
  and tuples (design 4.1).
- **Generic bodies.** `T < own Any` and `T < own Trait`, admission of
  owned arguments, inference of `T = own U`, modes after substitution,
  and one check per body for every allowed mode (design 4.6).
- **Closures and `dyn`.** One-shot `own fn` for owned and `^mut`
  captures, closures bounded by shared-borrow captures, and `own dyn K`
  with its drop (design 3.7, 4.6).

### Deferred

| Deferred | Why it waits | What the prototype does meanwhile |
| --- | --- | --- |
| Top-level owned bindings and borrows of top-level bindings (design 3.11) | Global moves, immovable borrowed globals and drop order across globals need the init-order analysis to follow drop glue (reviews U6, W4, Z5, Z6) | rejects both with a prototype diagnostic |
| Cancellation drops (design 3.1, 3.9) | Owned arguments in cold frames, owned results buffered by `all!` and `race!`, and partly evaluated calls need frame and combinator work (O6, U1, Z9) | rejects an owned argument to a cold call and an owned child result |
| `finish!` and other consuming cleanup methods (design 3.1) | They need the handle migration | nothing; ordinary consuming methods work |
| `Clone` and its derivation (design 6.3) | A std trait and a derivation template | nothing. When it lands, cloning a value whose type is annotated `own` deep-copies every field, plain fields included, so the copy shares nothing with the original and is fresh (owner, 2026-10-17) |
| Migration of std (design 6.6) | `^self` on std methods, `List`, `Map` and `Set` by structure, `IntoIter`, `retain`, handles | std methods keep plain receivers, so fixtures use their own types; `List[own T]` is `own-in-plain-type` |
| `defer` as a guard with no requirement row (design 4.5) | `defer-needs-row` would change programs without marks | `defer` keeps today's lowering; a `defer` suite that reads an owned local moves it at registration |
| Providers and the serialization boundary beyond rejection (design 3.10, 3.12) | Nothing more is needed | `provider-not-plain` and "not boundary-safe" are checked |

A fixture observes drops through a plain mutable top-level log that the
fixture prints at the end. A `drop` body runs in the restricted context
of a `defer` suite: it cannot suspend, and it cannot call `println` or
`block_on` directly.

## 2. Where It Lives

### The Opt-In

The prototype is built into the main compiler, behind an opt-in that the
spec does not expose. The recommended spelling is the environment
variable `HD_PROTOTYPE=ownership`, read once by the driver into its
options (question 1). Three properties follow.

- **Programs without marks are unaffected.** Every new check runs only
  on a body that uses a mark, and every new codegen path runs only for a
  type with drop glue. A program without marks emits the same Wasm as
  today; each slice that touches codegen reports the tiny-program
  footprint before and after, and any byte change blocks it.
- **Conformance stays green.** The portable suite never sets the
  opt-in, and no spec fixture changes.
- **The cache stays honest.** The opt-in enters the options field of the
  toolchain key ([hd_cache design](../../crates/hd_cache/DESIGN.md)),
  so every parse, check, code and link key differs under it. An opted-in
  build never reads a plain build's entries; the price is one cold build
  per toolchain with the opt-in.

The parser accepts the marks in every program, so the parse cache needs
no option; a mark without the opt-in is `ownership-prototype-off`,
reported once per module by the checker (question 3). No existing
fixture spells a mark, so no parse case changes.

### Fixtures

The fixtures follow the spec's fixture format
([conformance README](../../spec/conformance/README.md)) and live in
`crates/hd_driver/tests/ownership/`, with their own `cases.tsv` whose
specification column cites design sections (question 5). A driver test,
`crates/hd_driver/tests/ownership.rs`, runs them with the opt-in set,
using the conformance runner's machinery with a different suite root.
At the spec pass the fixtures move into `spec/conformance/` with rule
anchors; until then they are judged only by that test, and
`KNOWN_FAILURES.tsv` never lists them. The prototype's diagnostic codes
are the design's placeholder names, registered in `hd_diag` as a
prototype group that the spec-sync test skips.

### How The TIR Carries Ownership

The TIR stays the one typed IR of a body
([TIR design](../../crates/hd_tir/DESIGN.md)). The checker decides every
ownership fact and the builder records it, so collection and emission
never re-derive one.

| Fact | TIR form |
| --- | --- |
| A mode on a type | `TyData::Own(T)` and `TyData::Borrow { ty, mutable, lifetimes }` beside today's `Mut(T)`; lifetime arguments in `Adt` argument lists |
| A borrow | a `Coerce` of kind `Lend` or `LendMut` from a place's value to `^T` or `^mut T`; the design makes a borrow only at a `^` parameter or a `^` local (design 4.3), and both are coercion sites already |
| A move | a `LocalGet` or `Field` of an owned place that no `Lend` wraps; the ownership pass lists the move instructions in a `moves` column at `finish` |
| A drop at scope exit | a `DropLocal` registration in the enclosing `Scope`, beside `Defer`, emitted where the owned local is bound |
| A dropped temporary | a `DropTemp` at the end of its statement |
| Drop on overwrite | a `LocalSet` or `FieldSet` whose target the pass marks as possibly holding an owned value |
| A drop flag | a `drop_mode` column per `DropLocal`, written at `finish`: always, by flag, or never |
| Glue for a type | no TIR: collection maps a concrete type with drop glue to a generated body, as it maps `Builtin` choices today |

Putting `DropLocal` into the same `Scope` list as `Defer` buys three
things. Bindings and `defer` suites drop in one last-in, first-out
order (design 4.5). The exit ladder that runs `defer` suites on every
exit runs drops too. And the cancellation side record of each
suspension point already names the scopes whose suites cancellation
must run, so cancellation drops can come later without a TIR change.

These forms change TIR and `hd_types` for every program: new type
forms, coercion kinds, two tags and two columns. A plain body encodes
exactly as today, so its TIR hash and its code are unchanged; the wire
format version changes once (question 2).

### Which Stage Checks What

| Stage | Adds | Granularity | Superlinear risk | Incremental |
| --- | --- | --- | --- | --- |
| Parse (`hd_syntax`) | mark nodes; `hd fmt` support | per file | none: one token of lookahead decides a mark | parse key as today |
| Header lowering and resolution (`hd_resolve`, `hd_types`) | modes and lifetimes in signatures and fields; elision, `missing-lifetime`, `ambiguous-lifetime`; where an `OwnOnly` impl may be written | per item | none | modes and lifetimes are part of the interface hash, so a signature edit rechecks dependents and a body edit does not |
| Ownership by structure | is an instance `OwnOnly`; least fixed point over recursive types | per distinct type instance, memoized for the run like the selection table | deep generic nesting; bounded by type size, one visit per field per instance | a function of declarations, which the interface hash covers |
| Body checking (`hd_check`) | mode checks, freshness, `Lend` coercions, `DropLocal` registration | per expression, inline with inference | none: constant work per expression | per module, as today |
| Ownership pass (`hd_check::own`, new) | moves, drop flags, loans, conflicts, two-phase receivers, drop check | per body, after its types resolve and before the verifier | instructions times loans, in bitsets; loops iterate to a fixed point; each step charges body fuel, so a pathological body is `item-too-complex`, never a hang | reads only its own TIR and callee signatures, so it is exactly as incremental and parallel as body checking |
| Collection (`hd_mono`) | drop glue instances; a type parameter stays exact when its argument has glue | per instance | none | code keys as today |
| Emission (`hd_wasm`) | drop suites in the exit ladder; flag locals, saved in frames by existing liveness | per instance | none | code keys as today |

**Why the borrow checker runs over TIR, after types resolve.** Two
other places were considered. During syntax-directed checking, as
definite initialization works, a value's mode can still be an inference
variable, and non-lexical lifetimes need backward liveness, which a
forward walk cannot give. At emission, diagnostics would come too late
for `hd check`, and a generic body would be checked per instance, while
the design checks it once where it is written (design 4.6). Rust checks
borrows on MIR after type checking for the same reasons. The cost is
that checking now reads TIR for bodies with marks, which the checker's
design rule against reading TIR forbids today (question 4). Bodies
without marks skip the pass, so for them the rule still holds.

**The algorithm.** A lifetime is named by a variable, so a loan is a
place plus a kind, and every value carries a set of loans. A `Lend`
makes a loan; a call's result carries the loans of the arguments its
signature names (design 4.4); a field read through a borrow keeps the
base's loans. A loan is live while any live local or pending value
carries it, which is the non-lexical rule. One forward pass computes
maybe-initialized and maybe-moved bits for owned locals, one backward
pass computes liveness, and one forward pass computes the loans each
local carries. An access to a place then checks the live loans on
overlapping places. TIR's control flow is structured, so each pass is
linear in instructions times loop depth.

## 3. Slices

Each slice is about an hour of agent work and is one feature, in
dependency order. Every slice runs the usual gates (cargo fmt, clippy,
test, the portable conformance suite unchanged) plus the ownership
suite. **Spec impact is none for every slice** until the owner orders
the ownership spec pass; the slices below repeat it only where a reader
might expect otherwise.

| # | Slice | Needs | Done check | Risk |
| --- | --- | --- | --- | --- |
| 1 | Opt-in and suite | none | the suite runs one case; keys differ with and without the opt-in | low |
| 2 | Syntax | 1 | every design block parses; `hd fmt` is idempotent on them | medium |
| 3 | Modes and lifetimes in headers | 2 | header fixtures for each header diagnostic | medium |
| 4 | `OwnOnly` and `DropOwn` | 3 | fixtures for plain positions and fields | low |
| 5 | Ownership by structure | 4 | classifier tests and fixtures X2, X6, Y3, Z8, Z10 | medium |
| 6 | Mode checks in bodies | 5 | fixtures for upgrades, freshness, receivers | medium |
| 7 | Prototype limits and boundary rules | 6 | each deferred construct rejects | low |
| 8 | TIR forms | 6 | golden TIR of an owned body; std golden TIR unchanged | medium |
| 9 | Move checking | 8 | use-after-move fixtures, flags decided | medium |
| 10 | Drop emission | 9 | runtime fixtures print drop order; hello unchanged | high |
| 11 | Loans inside a body | 9 | borrow-conflict fixtures | high |
| 12 | Lifetimes across calls | 11 | elision, `lifetime-mismatch`, `borrow-escapes` fixtures | high |
| 13 | Two-phase receivers | 11 | X5, Y1, Y2 | medium |
| 14 | Lifetime parameters on data | 12 | `Parser`, `Export`, `Cursor` fixtures | high |
| 15 | Drop check | 10, 14 | U5, W1, Y7 | medium |
| 16 | Patterns and guards | 9 | U4, W1, Y8, Z2 | medium |
| 17 | Owned type arguments | 5, 9 | O3, U2, X1, W5, W6 | medium |
| 18 | Generic bodies for every mode | 17, 10 | X7, Y4, Y5, Y6, Z1, Z8; drops in `pick` | medium |
| 19 | Closures | 10, 12 | one-shot calls, O9, U9 | high |
| 20 | `own dyn` | 10 | a drop through a trait value | medium |
| 21 | `Option.take` | 16 | Z3; Z7 without cancellation | low |
| 22 | Suspending bodies | 12, 10 | children of `all!` borrow; Z4 without cancellation | medium |
| 23 | Tour and cost report | all | the tour's in-scope examples run; a cost table | low |

### 1. Opt-In And Suite

- **Work.** `HD_PROTOTYPE=ownership` into the driver's options and the
  toolchain key's options field. The `ownership.rs` driver test over
  `crates/hd_driver/tests/ownership/cases.tsv`, reusing the conformance
  runner with another root. The prototype diagnostic group in `hd_diag`.
- **Done when.** One `typing/valid` case passes; a driver test shows the
  check key differs with and without the opt-in; the conformance counts
  are unchanged.
- **Risk.** Low.

### 2. Syntax

- **Work.** Parse every form listed in section 1 in type positions, at
  `let`, at receivers, in generic parameter and argument lists, and in
  bounds. `^` in a type is a prefix; in an expression it stays XOR.
  `own` is contextual, so a binding named `own` keeps working. Formatter
  support. The checker reports `ownership-prototype-off` for a mark
  without the opt-in.
- **Done when.** The 35 blocks of the design's parse log, and every hd
  block of the six reviews, parse as written; parser snapshots pin the
  trees; `hd fmt` round-trips them.
- **Risk.** Medium: `[^]` and `[^{x}]` in argument lists, and `^{a}`
  beside a `{` block opener.

### 3. Modes And Lifetimes In Headers

- **Work.** The two new type forms in `hd_types`, their interning,
  interface encoding and display. Header lowering of modes in
  parameters, results, fields, enum payloads and type arguments.
  `own-on-value`, no `own mut`, `own-on-type-parameter`,
  `misplaced-lifetime` inside `Fn[...]`, `missing-lifetime`, and result
  elision with `ambiguous-lifetime` (design 4.1, 4.4).
- **Done when.** A `typing/invalid` fixture per diagnostic; an interface
  hash test shows a plain item's hash unchanged.
- **Risk.** Medium: the type encoding is shared by every stage.

### 4. `OwnOnly` And `DropOwn`

- **Work.** A std module declaring the marker `OwnOnly`, `DropOwn`
  extending it with `drop(^mut self)`, and `drop[T < own Any](x: T)`
  (question 6). The impl lives in the type's own module. A plain
  position of an `OwnOnly` type is `own-type-plain`, with fix-its to
  `own T` and `^T`; an `own` or `^mut` field of a type that is not
  `OwnOnly` is `own-field-plain-type`; calling `drop` directly is an
  error (design 4.1, 4.5).
- **Done when.** Fixtures for each rule, including the fix-its applied
  by `hd fix`.
- **Risk.** Low; std's interface hash changes once.

### 5. Ownership By Structure

- **Work.** `impl OwnOnly for Box[T] by Structure` and
  `@derive(OwnOnly)` as a compiler-known rule (design 4.1). Base cases:
  an `own` part or an `OwnOnly` type counts; a shared borrow and a plain
  function type do not; a `^mut` field needs a hand-written impl; an
  unused argument counts for nothing. Recursion takes the least fixed
  point. A hand-written unconditional impl also admits owned arguments.
  `Option` and tuples follow the rule built in. Anything else with an
  owned argument is `own-in-plain-type`.
- **Done when.** Unit tests of the classifier, and fixtures X2, X6, Y3,
  Z8 and Z10.
- **Risk.** Medium: memoization across recursive and mutually recursive
  types.

### 6. Mode Checks In Bodies

- **Work.** `own-upgrade` and `own-to-plain`; `let own` with the
  freshness rule, borrowed fields and primitives exempt; `:=` keeps
  `own` from an owned initializer; receivers follow parameters, so a
  plain `self` method on an owned value is `own-to-plain`;
  `mutable-upgrade` for `^mut` of a readonly plain place (design 4.1,
  4.3). The checker emits `Lend` and `LendMut` coercions. A private
  callable whose omitted result would be owned or borrowed must write
  its result in the prototype.
- **Done when.** Fixtures for each diagnostic, and accepted fixtures for
  the mixed owned and borrowed record of review U3.
- **Risk.** Medium: modes meet inference variables; a mode is decided
  where a binding takes its value.

### 7. Prototype Limits And Boundary Rules

- **Work.** A prototype diagnostic, `not-in-ownership-prototype`, for an
  owned or borrowed top-level binding, a borrow of a top-level binding,
  an owned argument to a cold call, and an owned child result of `all!`
  or `race!`. The design's own rejections that need nothing more:
  `provider-not-plain`, and owned or lifetime-carrying types are not
  boundary-safe (design 3.10, 3.12).
- **Done when.** One fixture per rejection; W3's `wrong` rejects.
- **Risk.** Low. Every later slice runs inside a subset whose holes are
  closed by rejection.

### 8. TIR Forms

- **Work.** The forms of section 2: the coercion kinds, `DropLocal` and
  `DropTemp`, the `moves` and `drop_mode` columns, builder calls,
  printer, wire form, and verifier invariants (a `DropLocal` names a
  local whose type may need a drop; a move names an owned place).
- **Done when.** A golden TIR file for a small owned body;
  `checker::std_bodies_match_golden_tir` passes without an update; the
  wire round-trip test covers the new forms.
- **Risk.** Medium: the wire form refuses unmapped words, so every new
  ID word must be mapped.

### 9. Move Checking

- **Work.** The ownership pass's first half, gated by the body's
  ownership bit: maybe-initialized and maybe-moved bits per owned local;
  `use-after-move` with a note at the move, naming the `defer` that
  moved it when one did; assignment re-initializes; no partial move and
  no move through a borrow (design 4.2, 4.3). It writes `moves` and
  `drop_mode`.
- **Done when.** Fixtures: the use after `archive(f)` (design 2), a
  move in one branch (accepted, flag chosen), a move inside a loop
  (rejected on the second iteration), a move then a reassignment
  (accepted), X8's moving `defer`.
- **Risk.** Medium: `break` and `continue` paths through nested loops.

### 10. Drop Emission

- **Work.** A drop glue body per concrete type: its `drop`, then its
  owned fields in declaration order, `Option` payloads and tuple
  elements. `DropLocal` suites in the exit ladder, with a flag local set
  at initialization and cleared at each move; overwrite drops; statement
  end drops; `drop(x)`. In `hd_mono`, a type parameter keeps its class
  only when the argument has no glue, so types without a destructor keep
  sharing one body (design 6.4).
- **Done when.** Runtime fixtures print the drop order for scope end,
  `return`, `break`, `continue`, `?`, overwrite, a flagged move and a
  temporary; review U10's reassignment drops the first file; the
  tiny-program footprint is byte-identical.
- **Risk.** High: the exit ladder, flags in suspending frames, and the
  folding change.

### 11. Loans Inside A Body

- **Work.** The pass's second half for loans made in the body: loans
  from `Lend`, loan sets carried by locals, liveness, and
  `borrow-conflict` on assignment, mutation, move and a conflicting
  borrow. A shared borrow copies and a mutable one moves; a field read
  through a borrow is a borrow; plain places are checked too (design
  4.3, 3.6).
- **Done when.** Fixtures: `reprice` (design 3.6), the reassigned
  document in `scan` (design 3.4) with a local view, a borrow that ends
  before the mutation (accepted), and review O5's plain capture
  (accepted by design).
- **Risk.** High: this is the core algorithm; its cost is measured in
  slice 23.

### 12. Lifetimes Across Calls

- **Work.** A call's result carries the loans of the arguments its
  signature names; elision with one input or a receiver; `^{a, b}`;
  `lifetime-mismatch` where a value outlives a source; `borrow-of-local`
  for a borrow of a parameter that dies with the call; `borrow-escapes`
  into a top-level binding, a plain position, or a result that does not
  name it; a store into another parameter writes its destination
  (design 3.3, 4.4).
- **Done when.** Fixtures: `busier` with and without `^{a, b}`,
  `dangle`, `remember`, and review O2's identity.
- **Risk.** High.

### 13. Two-Phase Receivers

- **Work.** A `^mut` receiver is reserved while its arguments run: reads
  pass, while mutation, moves and a `^mut` of anything that contains the
  place conflict. At activation the ordinary conflict check runs
  (design 4.3).
- **Done when.** `xs.push(xs.len())` with a user list type is accepted;
  X5's `holder.file.inspect_after(reset(holder))` and Y2's
  `file.compare(same(file))` reject.
- **Risk.** Medium.

### 14. Lifetime Parameters On Data

- **Work.** `data Parser[^doc]` with fields `^{doc} T`, enum payloads
  with lifetimes, construction carrying the field values' loans, use
  sites `[^{x}]` and `[^]`, and variance: covariant when readonly,
  invariant under `mut` or `^mut` (design 3.4, 4.4). A holder of a
  `^mut` field must be `OwnOnly`.
- **Done when.** Fixtures: `scan` (design 3.4), `Export` (design 3.4),
  W2's `Cursor` rejected by a plain generic, and `LinkError` from design
  5.
- **Risk.** High: lifetimes inside type arguments meet substitution.

### 15. Drop Check

- **Work.** A value whose type has drop glue uses every loan it holds
  when it drops, so each such loan lives until the drop (design 4.3).
  The implicit drop at a `DropLocal` counts as a use in liveness.
- **Done when.** U5's and W1's early `drop(file)` reject; Y7's
  `shared_cleanup` passes and `conflicting_cleanup` rejects.
- **Risk.** Medium.

### 16. Patterns And Guards

- **Work.** A pattern binding every field consumes an owned value; a
  private field moves out only in its module; a `DropOwn` type cannot be
  taken apart. In a guarded arm the bindings are shared borrows inside
  the guard and the move commits only when the body runs;
  `move-in-guard` (design 4.2). Emission keeps a precommit state and
  sets no move flag when a pattern merely matches.
- **Done when.** U4 and W1's `no_partial` reject; Y8 and Z2: `route`
  passes and its runtime fixture drops nothing on a false guard, while
  `consuming_guard` rejects.
- **Risk.** Medium: the decision tree binds locals before the guard.

### 17. Owned Type Arguments

- **Work.** The bounds `T < own Any` and `T < own Trait`. A plain `[T]`
  rejects an owned or `OwnOnly` argument, by structure, not by spelling.
  A call infers `T = own U`; `^T` with `T = own U` is `^U`; a type
  written with `T` is owned wherever its instance is `OwnOnly` (design
  4.6).
- **Done when.** O3 and U2's `duplicate` reject; X1's `take_first` over
  `Option[T]` returns an owned option; W5 and W6 work through slice 5's
  structure.
- **Risk.** Medium: modes after substitution must normalize to one
  canonical type.

### 18. Generic Bodies For Every Mode

- **Work.** A body under `T < own Any` is checked once: a value is
  move-only when its type can be owned under some allowed `T`, while
  `^T` and plain function types stay copyable (design 4.6). Locals of
  such types get `DropLocal`; their glue at an instance is empty when
  the argument has none.
- **Done when.** X7 and Y4 reject at the definition; Y5, Y6, Z1 and Z8
  pass; design 6.4's `pick` drops the value not returned exactly once,
  shown at runtime for a type with a destructor and one without.
- **Risk.** Medium.

### 19. Closures

- **Work.** A closure that captures an owned value moves it in and is
  `own fn`; so is one that captures a `^mut`. A call consumes it.
  Dropping it uncalled drops its captures, through glue for the closure
  environment. A shared-borrow capture bounds the closure by that
  lifetime. A plain capture stays as today (design 3.7, 4.6).
- **Done when.** Design 3.7's `tidy` rejects its second call; O9's
  `all!(send(), send())` rejects; an uncalled `on_close` closure drops
  its file at runtime; U9's lazy mapper rejects its second pull, and its
  named-data form with a `^mut self` step passes.
- **Risk.** High: capture modes, environment glue and one-shot calls
  meet in one place.

### 20. `own dyn`

- **Work.** `own U` converts to `own dyn K`; the vtable of a trait used
  this way gains a drop glue slot. A value holding a borrow converts
  only to `^{a} dyn K` (design 4.6).
- **Done when.** A runtime fixture drops a value through `own dyn K`;
  the tiny-program footprint is unchanged, since only traits converted
  to `own dyn` get the slot.
- **Risk.** Medium: vtable layout.

### 21. `Option.take`

- **Work.** The built-in `take()` on an `Option[own T]` place behind a
  `^mut`: it moves the payload out and leaves `None` (design 4.2).
- **Done when.** Z3's `detach` passes and the session's own `drop` still
  runs; Z7's `prepare_first` passes.
- **Risk.** Low.

### 22. Suspending Bodies

- **Work.** Borrows across bang calls. A cold frame carries the loans of
  its borrowed arguments, so two children of `all!` cannot hold one
  place when either borrow is mutable (design 3.9). Normal exits of a
  suspending body drop as in slice 10.
- **Done when.** Design 3.9's `mirror!` passes; the same with two `^mut`
  children rejects; Z4's guard over a suspending predicate passes
  without cancellation; a bang body's owned local drops on return.
- **Risk.** Medium: frames must save drop flags.

### 23. Tour And Cost Report

- **Work.** The in-scope examples of design sections 2 and 3 as runtime
  and typing fixtures, with the stubs they assume. Measurements: the
  tiny-program footprint, a cold `hd check` of the generated 10k-line
  plain package before and after the prototype, and an ownership-heavy
  generated body (thousands of loans, nested loops) to show near-linear
  growth. The results are appended to this record.
- **Done when.** The suite is green and the measurements are recorded;
  plain checking time is within noise.
- **Risk.** Low.

## 4. Hard Cases By Slice

The first review tested an earlier draft with no destructors, so its
cases are mapped by the shape they test under the current design.

| Case | What it tests | Slice |
| --- | --- | --- |
| O1 | late use after consuming the handle | 9, 10 |
| O2 | independent lifetimes through a plain identity | 12, 14 |
| O3 | generic duplication of an owner | 17 |
| O4 | an owned field in a plain holder | 4 |
| O5 | a plain capture mutates a borrowed variable | 11, accepted by design |
| O6 | owned results buffered by `all!` under cancellation | deferred; 7 rejects |
| O7 | leaving a consuming iterator early | deferred with std |
| O8 | clearing an owned collection | deferred with std |
| O9 | an exclusive capture in a reusable callable | 19 |
| O10 | an owned field beside a borrowed one; a later store of an aliased list | 6, 14; the store is question 7 |
| O11 | validate, then transfer | 9, 10; cancellation deferred |
| O12 | one mutable provider in two children | 7 |
| U1 | buffered owners and `clear` | deferred |
| U2 | generic duplication and owning boxes | 4, 17 |
| U3 | owned and borrowed fields, transfer after a wait | 6, 14, 22 |
| U4 | partial move then a custom destructor | 16 |
| U5 | destructor use after the last explicit use | 15 |
| U6 | static accessor over a movable global | deferred; 7 rejects |
| U7 | a `^mut` holder through a plain generic | 14, 17 |
| U8 | mutable providers in sibling tasks | 7 |
| U9 | a lazy mapper capturing an owner | 19 |
| U10 | reassignment drops first; `Option[T]` result | 10, 17 |
| W1 | partial moves and destructor-only uses | 15, 16 |
| W2 | a mutable-borrow holder is not plain | 14, 17 |
| W3 | exclusive services are parameters | 7 |
| W4 | an accessor freezes its global | deferred |
| W5 | generic result mode | 17 |
| W6 | nested field mode | 5, 17 |
| W7 | two owned payloads, one derivation | 5 |
| W8 | effectful `defer` | deferred (`defer-needs-row`) |
| X1 | optional result normalizes | 17 |
| X2 | nested fields and two-payload outcomes | 5 |
| X3 | the caller closes the cleanup's row | deferred (`defer-needs-row`) |
| X4 | moves and collection cleanup | 9, 10; `clear` deferred |
| X5 | receiver reservation | 13 |
| X6 | classifier base cases | 5 |
| X7 | generic copy of a maybe-owned wrapper | 18 |
| X8 | `defer` over an owner moves it; a borrowed view keeps it | 9, 11 |
| Y1, Y2 | reservation and activation | 13 |
| Y3 | structure stops at a shared borrow | 5 |
| Y4, Y5, Y6 | generic copies: rejected wrapper, accepted borrow and factory | 18 |
| Y7 | borrowed `defer` extends the loan | 15 |
| Y8 | guard failure keeps the candidate | 16 |
| Y9 | replacement through `^mut` | 9 rejects the naive form; the `List` form is deferred |
| Z1 | known modes copy in generic bodies | 18 |
| Z2 | guard failure preserves the owner | 16 |
| Z3 | `take` inside a destructor-bearing value | 21 |
| Z4 | suspension inside a guard | 16, 22; cancellation deferred |
| Z5, Z6 | destructors reading globals at shutdown | deferred with globals |
| Z7 | `take` before a suspension | 21; cancellation deferred |
| Z8 | mode cutoffs through containers | 5, 18 |
| Z9 | an owned argument before a suspending sibling | deferred; 7 rejects the cold-call form |
| Z10 | an always-owned generic | 5 |

## 5. Questions For The Owner

Each question is a choice the design leaves to the implementation, or a
gap the plan found. Each gives the recommendation first.

1. **The opt-in's spelling.** Recommended: the environment variable
   `HD_PROTOTYPE=ownership`, read into the driver's options and the
   toolchain key. The alternatives are a hidden `--prototype ownership`
   flag or a manifest key; both touch command-line rules the spec owns.
2. **TIR and types for every program.** May the prototype add the new
   type forms, coercion kinds, tags and columns to the shared TIR and
   `hd_types`, with plain bodies encoding exactly as today and the wire
   format version changing once? Recommended: yes. The alternative is a
   side structure that only opted-in bodies carry, which emission would
   have to merge.
3. **Parsing the marks.** May the parser accept the marks in every
   program, with a mark outside the opt-in reported by the checker as
   `ownership-prototype-off`? Recommended: yes, which keeps the parse
   cache free of options. The alternative gates the parser itself.
4. **The borrow checker reads TIR.** For bodies with marks, the
   ownership pass reads the body's TIR after types resolve, which relaxes
   the rule that checking never reads TIR. Recommended: accept it for
   those bodies. The alternative has the checker record its own table of
   events and runs the pass over that.
5. **Where the fixtures live.** Recommended:
   `crates/hd_driver/tests/ownership/`, in the spec's fixture format with
   design citations, moving into `spec/conformance/` at the spec pass.
6. **Std declarations.** May `OwnOnly`, `DropOwn` and `drop` land in a
   new std module that every program can see but only an opted-in
   program can use? Recommended: yes; it changes std's interface hash
   once and no program's code.
7. **Stores into an owned value's plain fields.** The freshness rule
   covers construction (design 4.1). Review O10 assigned a pre-aliased
   plain list into a plain field of an owned value afterward. Does the
   same freshness rule apply to that store? Recommended: yes, so `own`
   keeps meaning the only path to the whole value; a later relaxation
   breaks nothing.
