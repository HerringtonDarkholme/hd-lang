# One Lifetime Solver

**Status:** proposal, 2026-10-30; domain: the tier-2 lifetime checker
(compiler internals); not accepted. It reviews `crates/hd_check/src/dep_results.rs`
and `local_refs.rs` at e77e0c2d3, the stages in
[The Path To Returned Local References](RETURNED_REFERENCES_PATH.md), the
cost figures in [the tier-2 study](OBJECT_MODEL_TIER2.md#13-costs) and the
#978 log. It changes no spec, compiler or std code. Tier 2 itself is
committed; this is about building it cheaper.

## The proposal

Replace the per-shape origin propagation in `dep_results.rs` with Rust's
shape: every reference position of every value in a body gets a region
variable, each TIR instruction adds outlives constraints between them, and
one solver computes all region values before the checks read them. Body
types stay erased: the region variables live in a side table that runs
parallel to each type's reference slots, which the struct lifetime
arguments (`lifetime_arg`) already reserve. The type pool, the interface
cache are untouched, and the spec changes only where the owner drops one
of its unions. The work is eight slices, none adding more than about 330
lines; it ends before stage 7 and leaves conformance green and the bench
byte-identical after each slice.

```text
struct Two['a, 'b]:
    x: &'a string
    y: &'b string

struct Outer['a, 'b]:
    inner: Two['b, 'a]

fn set_through['a, 'b](t: &mut Outer['a, 'b], s: &'b string) -> void:
    r := &mut t.inner   # r: &'r mut Two[ρ1, ρ2]; ρ1 = t's 'b, ρ2 = t's 'a
    r.x = s             # s's region flows into ρ1 = 'b: valid
```

Today `r.x = s` is a false `local-reference-escapes` (#1039): `r`'s value
keeps one origin set for its reference, so the store through it can only
check `Outer`'s whole referent. With region slots, `&mut t.inner` gives `r`
the same variables as the place `t.inner`, and the store checks `'b`.

Two choices need the owner. First, the spec's deliberate unions
(`ref.dep.origin.position.union`, the "one lifetime in all positions" of
`ref.lifetime.store`, `ref.generic.call.unaligned`) become explicit merge
constraints in the new solver, so
the switch changes no verdict; each union is then dropped in its own
owner-approved slice with a spec edit (**Recommendation:** drop them all
after the switch, one per slice). Second, the solver stays
location-insensitive, as `ref.dep.origin.set` specifies; stage 7 adds
liveness on the same graph (**Recommendation:** keep it insensitive until
stage 7).

## Problems it solves

**1. Every slice adds a propagation path, and each path finds the next
false rejection.** In stage 6's checker commits, `dep_results.rs` and
`local_refs.rs` took most of each compiler diff (6b1: 224 of 277 lines,
6d5: 471 of 489, the `fn` parameter captures: 430 of 498). Since tier 2
started, those two files saw 4,351 lines added and 1,154 deleted.
Each path lines a value's sets up with a type by hand (`field`, `range`,
`slot_in`, `lifetime_args`, `generic_store`, `caller_place_takes`), and
each falls back to "the union of every position" where its shape test
fails. With one solver, a new construct is one row of constraint
generation; precision comes from the types, not from the path.

**2. A reborrow loses its positions.** The first function is rejected
today, and Rust accepts it; the second differs only in how `r` is bound:

```text
struct Two['a, 'b]:
    x: &'a string
    y: &'b string

fn via_field_ref['a, 'b](t: &Two['a, 'b]) -> &'a string:
    r := &t.x
    *r          # today: "borrows from `t` with lifetime `'b`"

fn via_copy['a, 'b](t: &Two['a, 'b]) -> &'a string:
    r := t
    r.x         # valid today and in the proposal
```

`via_field_ref` fails because a `Ref` value has one origin set, so `*r`
reads every slot of `t`'s referent. `via_copy` passes because `r` keeps
`t`'s sets slot by slot. The store-side twin is #1039's
`r := &mut t.inner; r.x = s`. In the proposal a reference's inner slots
*are* its place's slots, so both read and store keep their lifetime. The
store is already specified as valid; the read follows the spec's union of
positions today, so it is one of the owner-approved relaxations below.

**3. Shapes the propagation skips are unchecked.** `compose` (#1041)
returns a closure that captures two `fn(...)` parameters, and today
nothing checks the result's capture slot:

```text
fn compose(f: fn(i32) -> i32, g: fn(i32) -> i32) -> fn(i32) -> i32:
    fn(x: i32) -> i32: g(f(x))      # accepted today; no single source
```

In the proposal every capture flows into the result's capture slot, whose
region is the elided one. With two `fn(...)` parameters elision has no
single lifetime, so it is `ambiguous-reference-source`, as #1041 asks. No
shape can be skipped, since every instruction adds its constraints.

## How it works

**Region slots.** A type's *slots* are what `slots_of` lists today: each
local `&` and `&mut` outside a function type or a counted reference, each
struct lifetime argument (the `&void` placeholder `lifetime_arg`), each
function type's capture, each borrowing opaque type, and one for a
`&dyn` value's data. Each value and each local of a body gets one region
variable per slot of its type, numbered in a side table (`base[v]` plus
the slot index). Body types stay erased, so `Ty` equality, the pool and
the cache are unchanged.

**Two cached maps.** A *slot layout* per distinct type (its slots in
preorder, each with its kind and the end of its subtree) replaces the
repeated `slots_of`/`tops` walks. A *field slot map* per struct field and
variant payload says, for each slot of the field's declared type, which
slot of the parent instance it is: a type argument's slot, or the slot of
the lifetime it names. `Outer.inner: Two['b, 'a]` maps to `Outer`'s second
and first lifetime slots, at any depth.

**Loans** are today's origins: what a parameter slot refers to, a local, a
hidden local of `&e`, and a top-level binding. A region's value is a
bitset of loans. Stage 7 later splits a local's loan per `&` site and
path.

**Constraints**, written `a ⊑ b` for "everything `a` may point into, `b`
may too" (Rust writes `'b: 'a`). A slot is invariant under a `&mut`, and
in a struct lifetime that a `&'a mut` field names, as Rust infers
variance; elsewhere it is covariant:

| TIR construct | Constraints |
| --- | --- |
| `Ref` of place `p` | slot 0 holds the loan of `p`'s root; slots 1.. are `p`'s slots, both ways |
| `Read` of `p` | `p`'s slots ⊑ the value's, both ways in invariant slots |
| `Store` into `p` | the value's slots ⊑ `p`'s, both ways in invariant slots; a universal slot of `p` adds a check |
| field, variant, tuple projection | the field slot map |
| `Deref` projection | the reference's slots after slot 0 |
| `Elem`, `push`, map store | the collection's type-argument slots |
| struct, variant, tuple, list construction | each part ⊑ the parent, by the field slot map |
| `Coerce` | slot by slot; to `&dyn` or a shape that does not line up, every slot ⊑ each target slot |
| block, `if`, `match`, loop, `break` | each arm's value ⊑ the result |
| call | fresh variables per callee lifetime and per type-parameter slot; argument ⊑ parameter; a `&mut` parameter's inner slots equal the argument's; the result takes its lifetime's variable |
| closure | each captured local's slots ⊑ the capture slot |
| return, body value | value ⊑ the result's universal regions (checked) |
| `box`, provider, host call, `'static` type parameter | value ⊑ `'static` (checked) |

The table covers every tag that `Solve::step`, `generic_store` and
`captured_fn_escapes` handle today. The call row replaces the `TVAR`,
`ANY` and `ANY_FN` encodings: `line_up` still pairs each parameter with its
argument, and a type parameter's subtree gets one fresh variable group.

**One solver.** Build the constraint graph, collapse strongly connected
components with an iterative Tarjan pass, and union bitsets once in
topological order. It is location-insensitive, as `ref.dep.origin.set`
specifies: one value per region for the whole body, as rustc's NLL computes
for its universal regions and Polonius's location-insensitive variant for
all of them. Stage 7 adds liveness on the same graph.

**Checks and errors.** Each signature lifetime is a universal region.
Its parameter slots start with their placeholder loans, and implied bounds
come from the slot layout (a slot behind `&'a` outlives `'a`, as `allows`
says today). A universal region may hold only placeholders of lifetimes
that outlive it, and `'static` only top-level bindings. Every checked edge
carries its instruction and a category (result, caller's place, `box`,
provider, host call, capture into a field, `'static` parameter). The
category picks today's message and code, the instruction picks the line,
and `site` still narrows a returned value to its arm. The scope check
(`local-reference-outlives-local`) keeps its syntax ranges and reads the
solved values. No code or message changes.

**Spec unions as hubs.** Where the spec asks for a union of all
positions (`ref.dep.origin.position.union`, the whole-referent case of
`ref.lifetime.store`, `ref.generic.call.unaligned`), the generator routes
every slot of that value through one hub variable. So the switch changes
no verdict, and removing one merge is one slice and one spec rule.

## Cost

**Bottom-up total**, the stop-at-1.5x rule applying to the build:

| Component | Added | Deleted |
| --- | --- | --- |
| `hd_check`: the solver and its generators | 1,180 | 2,060 |
| `hd_check`: shadow comparison (S5, deleted at S6) | 120 | 120 |
| `hd_check`: unit tests of the solver | 150 | 0 |
| `hd_resolve`, `hd_types`, `hd_syntax` | 0 | 0 |
| fixtures: the closed gaps and the variance cases | 160 | 0 |
| spec ch. 13: one rule per dropped union, if approved | 40 | 60 |
| `lib/std`: `compose` names its lifetime (#1041) | 5 | 2 |

Net about −900 checker lines; the two lifetime files go from 3,852 to about
2,970 (Appendix E maps each function). The larger gain is per future
slice: a construct becomes one row of constraints (20 to 60 lines) instead
of a path and its fallback (40 to 500 measured per stage-6 slice).

**Compile time.** The lifetime passes take about 10% of body checking on
the fixtures and 3% to 13% on std modules, about 1% of a whole `hd check`
(Appendix D). Most of it is not the fixpoint: 95% of bodies never run it,
yet `dep_results` takes 8.4% of their body checking, in the scan that
builds signature maps before deciding to skip. The
proposal keeps one skip test, a per-type flag cached in the slot layout
("this type has slots"), and builds nothing for a body whose values and
locals have none. Where it does run, solved bodies average 0.32 slots per
instruction, so the region table is a fraction of the instruction count.
The work is linear in slots plus edges, with no pass count (Appendix E).

**Migration**, one checkable step per slice, each keeping all three
conformance modes green and the bench byte-identical (the checker emits no
code):

| Slice | Step | Lines | Check |
| --- | --- | --- | --- |
| S1 | slot layouts and field slot maps; today's solver reads them | +220 −270 | diagnostics identical on every fixture and `lib/std` |
| S2 | region table, constraint graph, SCC solver, with unit tests; not wired | +330 | unit tests |
| S3 | constraints for places, stores, constructions, control flow, with variance | +260 | shadow report: each instruction's loans against today's set |
| S4 | constraints for calls and closures, capture destinations | +310 | shadow on all calls |
| S5 | universal regions, checks and categories in shadow | +330 | every verdict difference on fixtures and `lib/std` listed; none left unapproved |
| S6 | switch: diagnostics come from the new solver; delete `Solve` and the paths | +20 −1,910 | conformance green |
| S7 | #1039, already specified as valid: drop that store's hub | +60 | valid and invalid fixtures |
| S8 | #1041: a result capture slot with no single source is ambiguous; `compose` names its lifetime | +40 | invalid and valid fixtures |
| S9+ | one slice per spec union the owner drops | ±40 each | spec rule plus fixture flip |

Stage 7 then adds per-site loans with paths and kinds, liveness over the
same graph, and the conflict check.

## Appendix A: Stress test

Each case was traced by hand through the constraint table; "today" is a
probe with the release build at e77e0c2d3.

| Case | Today | Proposal |
| --- | --- | --- |
| `r := &mut t.inner; r.x = s` (#1039) | false `local-reference-escapes` | valid: `r`'s inner slots are `t.inner`'s |
| `r := &mut t.inner; q := &mut r.x; *q = s` | false `local-reference-escapes` | valid: `q`'s inner slot is `t`'s `'b` slot |
| `r := &t.x; *r` returned as `&'a` | false `local-reference-escapes` | valid |
| `compose(f, g)` capturing both (#1041) | unchecked | `ambiguous-reference-source` at the signature |
| `swap_in(&mut xs, &mut ys)` over one `'a` | valid | valid: one fresh `'a` per call joins both lists' slots, as today |
| `let mut r = a`, reassigned inside a loop | valid | valid; a reassignment still adds and never removes (location-insensitive) |
| recursive `struct Node['a]` with `next: (&'a Node['a])?` | valid | valid: a layout is per type, not per field, so it is finite |
| a 30,000-line body (`inferred-results-a-long-chain…`) | `dep_results` 5.4% of its body checking, all in the scan; no fixpoint | linear: one pass to generate, one SCC pass, one propagation |
| std `xs.iter().map(fn(x): x + *step)` with `step: &i32` (6d5's shape) | valid; `MapIter[I, T, U, F]` carries the capture | valid: `F`'s slot group is one fresh variable shared by the argument and the result |
| std `json.hd` `fn top(&mut self) -> &mut TextReadFrame`, an element of a list field | valid | valid: the element's loan is `self`'s placeholder through the `Elem` projection |
| `box` of a closure that captures a local | `local-reference-escapes` | the same: the capture slot ⊑ `'static` |
| `&dyn Debug` of a `Two['a, 'b]` | one set for the trait value | one dyn slot; every slot of `Two` ⊑ it |

**Found: stores need variance.** Tracing `let mut r = &mut x; *r = s`
with plain `⊑` from the `Ref` value into the local `r` loses the write:
`s` reaches `r`'s inner slot, not `x`'s. Today's checker avoids this by
writing into the storage of `r`'s origins. Rust's answer is invariance:
slots under a `&mut` relate both ways. So `relate` takes a variance per
slot: covariant by default, invariant under `&mut` and in a struct
lifetime that a `&'a mut` field names (computed once per declaration, as
Rust computes variance).

**Found: invariance can merge more than today.** With
`let mut r = &mut x` and later `r = &mut y`, invariance makes `x`'s and
`y`'s inner slots one region, so a loan stored into `x` also shows in `y`.
Rust's NLL has the same merge (its type regions are location-insensitive).
Today's checker keeps them apart. This is the one place the switch could
reject a valid program; the shadow slice (S5) measures it over every
fixture and `lib/std` before anything switches.

**Stays open: function types carry no slots for their parameters and
results.** A type parameter named inside a function type (`fn() -> F`)
still has no position (`ref.generic.outlives.argument`'s "only bare").
Closing it needs regions inside function types, Rust's higher-ranked
`for<'a>`; that is not part of this design.

## Appendix B: Prior art

**Rust, NLL (shipped 2018).** The borrow checker renumbers every region in
a MIR body to a fresh variable, the type checker emits outlives constraints
with a category per statement, and region inference solves them once:

```rust
struct Two<'a, 'b> { x: &'a String, y: &'b String }

fn via_field_ref<'a, 'b>(t: &Two<'a, 'b>) -> &'a String {
    let r = &t.x;   // r: &'r &'a String, 'a taken from t's type
    *r              // constraint 'a: 'result, holds
}
```

rustc collapses the constraint graph into strongly connected components and
propagates in dependency order (`ConstraintSccs`); errors pick the "best
blame" constraint by its `ConstraintCategory` (return, assignment, call
argument, boxing). hd takes the renumbering, the categories and the SCC
solve, and keeps regions out of its interned types. Sources:
[region inference](https://rustc-dev-guide.rust-lang.org/borrow-check/region-inference.html),
[RFC 2094](https://rust-lang.github.io/rfcs/2094-nll.html).

**Polonius, location-insensitive variant.** Polonius states the same
analysis as Datalog; its cheapest variant drops program points:

```prolog
subset(Origin1, Origin2) :- subset_base(Origin1, Origin2, _).
origin_contains_loan_on_entry(Origin, Loan) :- loan_issued_at(Origin, Loan, _).
origin_contains_loan_on_entry(Origin, Loan) :- placeholder_loan(Origin, Loan).
origin_contains_loan_on_entry(Origin2, Loan) :-
    origin_contains_loan_on_entry(Origin1, Loan), subset(Origin1, Origin2).
```

That is exactly hd's specified "once per function, regardless of the order
of statements" (`ref.dep.origin.set`), with loans in place of origins. hd
takes this variant for stages 1 to 6. Source:
[location_insensitive.rs](https://github.com/rust-lang/polonius/blob/master/polonius-engine/src/output/location_insensitive.rs).

**Swift, lifetime dependence diagnostics.** Swift finds the roots of
`~Escapable` values and "walk[s] down all dependent values checking that
they are within range", a def-use walk per dependence in SIL:

```swift
@_lifetime(borrow array)
func span(of array: [Int]) -> Span<Int> { array.span }
```

That is the shape of hd's checker today, a path per value kind. It works
for Swift because dependence is per call result and exclusivity carries the
rest. hd has stored references in structs and through `&mut` stores, where
a per-value walk needs a path per container shape. Source:
[LifetimeDependenceDiagnostics.swift](https://github.com/swiftlang/swift/blob/main/SwiftCompilerSources/Sources/Optimizer/FunctionPasses/LifetimeDependenceDiagnostics.swift).

## Appendix C: Alternatives considered

- **Regions inside `TyData`, as rustc.** Every `Ref` and lifetime argument
  would carry a region, and bodies would intern fresh ones. The pool is
  append-only and shared by all threads through sharded locks, so each
  body's variables would stay for the session and contend. Every `Ty`
  comparison in `hd_check` (45,000 lines) would need erasing first, which
  is why rustc erases regions before its type-based queries. The side
  table gets the same precision with no pool change.
- **Keep extending the paths.** The remaining stage-6 slices (6c, 6f to
  6i), #1039 and #1041 each add a path and its fallback. Measured so far:
  40 to 500 compiler lines per stage-6 slice, mostly in the two lifetime
  files, with 6d5 rewriting 303 of them.
- **Location-sensitive NLL now.** Liveness per program point needs a
  control-flow view of TIR's structured blocks, about 600 to 900 more
  lines, and it changes which programs are valid. Stage 7 needs it; stages
  1 to 6 do not.
- **Unification only (equality, near-linear).** Union-find merges both
  directions, so `if c: a else: b` would give `a` the loans of `b`. It
  would reject valid programs that today's sets accept.
- **A Datalog engine.** Polonius's own engine is a dependency and slower
  than a direct SCC pass on bodies of tens of slots.

## Appendix D: Today in hd

**Check time, tier 2 so far.** `hd check FILE` with the release builds of
8047441b6 (just before tier 2, `5a155e7ba^`) and e77e0c2d3, interleaved per
file, `HD_JOBS=1`, child CPU time (user plus system) from `getrusage`. The
machine was shared: load average 20 to 45 on 14 cores, so wall times are
noisier than CPU times. The range also holds non-tier-2 commits (const
generics, map lookups), so the ratios bound tier 2 from above.

| Input | Before tier 2 | Now | Ratio |
| --- | --- | --- | --- |
| 6,071 conformance fixtures, one cache per run, CPU, median of 3 | 208.0 s | 214.2 s | 1.030 |
| the same, wall | 274.1 s | 283.1 s | 1.033 |
| the 3,268 fixtures both accept, sum of per-file CPU medians | 99.6 s | 102.2 s | 1.026 |
| 22 bench programs, a fresh cache per check, CPU, median of 11 | 1.41 s | 1.44 s | 1.022 |
| one bench program | 62 to 65 ms | 64 to 67 ms | 1.01 to 1.05 |

Of the fixtures, 3,268 check with both builds, 124 only now (tier-2
syntax), 25 only before (rules tier 2 made errors), and 2,654 with neither
(invalid fixtures).

**The lifetime passes' share.** Temporary timers (not committed) around
`check_dependent_results`, `check_local_ref_types` and the
`local_refs::declarations` walk, against the time in function-body checks,
`HD_JOBS=1`, one fresh cache per file:

| Input | `dep_results` | `local_refs` | Bodies that ran the fixpoint |
| --- | --- | --- | --- |
| 5,091 fixtures with timed bodies, summed | 10.3% of body checking | 0.6% | 4.9%, 1.7 passes each (22 at most) |
| the same, median per file (90th percentile) | 12% (26%) | | |
| the 194 tier-2 fixtures among them | 26% | 0.8% | 46%, 2.1 passes |
| std modules checked as files: `json`, `http`, `regex`, `format`, `serde`, `iter` | 7.0%, 3.4%, 4.9%, 13%, 4.6%, 7.6% | 0.3% to 0.8% | 0 to 20 bodies each |
| the 30,000-line fixture (10,001 bodies) | 5.4% | 0.4% | none |

A `samply` profile (8 kHz, three runs each of `json`, `http` and `format`)
agrees: `check_dependent_results` holds 0.67% of all samples and 5.7% of
the samples under `check_fn_body`, and `local_refs` 0.24%. Inside it the
scope check is 18%, `step` 14% and `captured_fn_escapes` 10%. A whole
`hd check FILE` spends most of its time lexing and loading std, so the
lifetime passes are about 1% of the command. Most of their time goes to
bodies that hold no local reference: in fixtures whose bodies never ran
the fixpoint, `dep_results` still took 8.4% of body checking, the
scan that runs before the skip test (`store_dep`, `callee_dep`, the
body's own `DepSig`).
980 fixtures are left out: no body checked, or a timer preempted under
load (a reading longer than its enclosing body).

**Lines added by tier 2**, from `git log 8047441b6..e77e0c2d3`, the 63
tier-2 commits with their design records (const generics, map work,
server SDK refreshes and cleanups left out):

| Component | Added | Deleted | Net |
| --- | --- | --- | --- |
| `hd_check` `dep_results.rs` (new) and `local_refs.rs` | 4,351 | 1,154 | 3,197 |
| `hd_check` elsewhere (`call.rs`, `body.rs`, `expr.rs`, `ty.rs`, tests) | 1,050 | 279 | 771 |
| `hd_resolve` (lifetime scopes, elision, slot tables) | 1,074 | 193 | 881 |
| `hd_types` (`FnDeps`, `'static`, wire format) | 491 | 25 | 466 |
| `hd_syntax` (lifetime tokens and parameters) | 345 | 22 | 323 |
| backend (`hd_tir` frame slots, `hd_opt` inliner, `hd_wasm_linear`) | 751 | 210 | 541 |
| other crates (driver tests and baselines, lint, diagnostics) | 990 | 221 | 769 |
| compiler crates in all | 9,052 | 2,104 | 6,948 |
| `lib/std` | 515 | 410 | 105 |
| spec ch. 13 (rules 161 to 280; 750 to 1,505 lines) | 910 | 155 | 755 |
| spec, other chapters | 607 | 193 | 414 |
| conformance fixtures (219 new cases) | 4,161 | 818 | 3,343 |
| design records in `docs/` | 1,151 | 168 | 983 |

## Appendix E: Details

- **Pipeline position.** After a body is type-checked and zonked, where
  `check_local_ref_types` and `check_dependent_results` run now
  (`body.rs`, `finish_with`). It reads TIR and interface signatures only.
- **Invariants.** Body types stay erased; slot order is `slots_of`'s
  preorder; a value's region variables are `base[v]..base[v] + slots`; the
  only cross-body input is `FnSig::lifetimes`, field lifetimes and
  `FnDeps`, as today.
- **Data layout.** A body-local `HashMap<Ty, LayoutId>`; field slot maps
  cached per declaration for the session; constraint edges as
  `(from, to, category, instruction)` in one `Vec`; bitsets of
  `ceil(loans / 64)` words per component.
- **Worst case.** Linear in slots plus edges. A spec union over `k`
  slots costs `2k` edges through its hub, not `k²`.
- **Determinism.** Variables, edges and components are numbered in
  instruction order, so diagnostics keep today's order.
- **Diagnostics.** Same codes and messages: `local-reference-escapes`,
  `local-reference-outlives-local`, `reference-result-without-source`,
  `ambiguous-reference-source`. No fix-its.
- **Tests.** Solver unit tests on hand-built graphs; the shadow comparison
  over every fixture and `lib/std` in S3 to S5; conformance in three modes
  per slice; the 22 bench programs byte-identical.
- **Rollback.** Until S6 the old solver decides; S6 is one revertible
  commit.
- **Stage 7 hook.** Loans gain a path and a kind per `&` site; liveness per
  region over TIR's structured blocks; a loan live at a conflicting
  access is `conflicting-access`.
- **Parse log.** All three `text` blocks parse with
  `scripts/hd-conformance.sh parse` at e77e0c2d3, and `hd check` on that
  build reports what their comments say: block 1 at `r.x = s`, block 2 at
  `*r`, block 3 accepted.

**Compile-time dimensions.**

| Dimension | Today | Proposal |
| --- | --- | --- |
| Work per body | passes × instructions × a slot walk with allocations | slots + edges, once; SCC and propagation linear in edges × loan words |
| Allocation | `Vec` sets per slot, cloned per read | one `u32` base per value, one bitset per component |
| Worst case | a long dependency chain re-runs the whole body per pass | linear; no pass count |
| Bodies with no local reference | scanned (8.4% of their body checking), then skipped | skipped by one cached per-type flag |
| Pool, cache, incremental | none | none: body types stay erased |
| Run time, Wasm size | none | none |
| Web compiler gzip | +51 KB for tier 2 so far (#978) | expected to shrink with the net −900 lines; measured at S6 |

**What it replaces**, by today's functions in `dep_results.rs` (2,889
lines) and `local_refs.rs` (963):

| Today | Lines | Becomes | Lines |
| --- | --- | --- | --- |
| `slots_of`, `tops`, `top_runs`, `width`, `dep_slots`, `written_fns` and kin; `field`, `range`, `local_range` | 270 | slot layouts and field slot maps | 220 |
| `Origin`/`Set` helpers, `Solve`, `held`, `hold`, the `while changed` loop | 180 | region table, SCC, bitset propagation | 180 |
| `step`, `place`, `project`, `store`, `intrinsic_store`, `construct`, `lent`, `arg_range` | 360 | constraints for places, stores, constructions, control flow | 260 |
| `instantiate`, `lifetime_args`, `slot_in`, `reach`, `generic_store`, `call`, `store_dep`, `callee_dep`, `caller_place_takes`, `TVAR`/`ANY` | 420 | constraints for calls, with variance | 190 |
| `call_params`, `captured_fn_escapes`, `FnDest`, `fn_origins`, `outlives_fields`; `scoped_param`, `capturing_field`, `outlives_field` | 400 | capture and `'static` destinations as regions | 120 |
| `check_dependent_results`, `allows`, `result_ids`, `inputs`, `all_ids` | 430 | universal regions, checks, categories | 210 |
| `DepSig::new`/`of_fn`/`to_fn`/`open`, `line_up`, `fn_deps_fit` and kin; messages, `site`; the scope check | 940 | kept, adapted | 940 |
| `local_refs.rs` declaration walk, `lends`, kind coercions, `check_local_ref_types` | 850 | kept | 850 |
