# Ownership Per Instance

Status: accepted design, 2026-11-04; domain: compiler internals (ownership, counts,
storage); accepted by the owner on 2026-11-04, including the reopened
decision below. The owner picked this direction (Option 1 of the
brainstorm, in this file's git history at d1e8f3bc5) on 2026-11-04. Nothing
here is in the specification, and it changes no spec rule. It reviews
`crates/hd_tir/src/own.rs` and `own/*`, `crates/hd_driver/src/own.rs`,
`instance_body` in `crates/hd_driver/src/lib.rs`, the emitter's ownership
code in `crates/hd_wasm_linear/src/emit/*` and `opt.rs`,
[TIR v2](../compiler/tir-v2.md#the-ownership-passes) and the
[TIR v2 directions](../compiler/tir-v2-directions.md#fixed-constraints).
The evidence is the static audit
[#1364](https://github.com/HerringtonDarkholme/hd-lang/issues/1364) and its
115 feature snapshots in `crates/hd_driver/tests/cases/features/`.

## Where TIR v2 Falls Short

The owner, 2026-11-04: "we still need a solid foundation, this is the one
we need to do. TIR is better, but still fall short". TIR v2 gave the
compiler places, reads with a mode, and `Drop` and `IsUnique` statements.
As a base for ownership it still lacks seven facts, so the passes guess or
match patterns, and later layers patch what they missed.

| Fact the passes need | What TIR v2 has instead | What the passes and later layers do |
| --- | --- | --- |
| whether a value counts | one generic body; a type parameter is `Tri::Maybe` (`OwnCx::counted`, `hd_tir/src/own.rs`) | keep a `Drop` for "maybe"; the emitter deletes count glue for scalar instances |
| which names are one value | every local is a place | `bind` moves `s` through `$1`, `$2`, `$3` and ends `summary kept [$0]`; `late.rs` reorders reads so one is "last" |
| every count, as an instruction | a read mode implies a count; exits imply drops | the emitter adds five kinds of glue the body cannot show (header of `emit/count.rs`: `release_untaken`, `push_args`, entry retains, instance-owned releases, unwind releases) |
| whether a buffer is unique | nothing | an `IsUnique` before every change, then `hoist.rs` moves it out of loops by pattern; "a call with a counted result counts as a write" (#1371) |
| where storage lives | a `ref_word::FRAME` bit for at most 64 candidates (`own/frame.rs`) | any other addressed or shared local becomes a counted cell, a `Fn` value always escapes, and the emitter builds environments (`emit/closure.rs`) and promotes arguments (`emit/promote.rs`) |
| where a loan ends | the holder's `Drop` | a reference that counts nothing keeps a release call, so its loan can end (#1360) |
| what is live at a suspension point | nothing | `emit/susp.rs` saves every local at every point (#1393) |

The result is six layers that each decide some counts: the inliner, the
TIR passes, the release summaries, the emitter glue, the Wasm cleanup's
count rules (`opt.rs`: `static_values`, `static_counts`) and
a link-time fold of `rt:ref_is_view`. Insertion makes every read a copy.
Five passes then weaken it, each under its own syntactic rule. The audit's
classes are the shapes no rule matched, and nothing tells us when the
last one is found.

**What this design adds** is each missing fact, as TIR content written
before counts are decided: concrete types per instance; values for
never-addressed locals; one decision per count with its reason; a
uniqueness fact; a storage mark; explicit loan begin and end; and live
sets at suspension points.

## The Proposal

Decide every count once, in one TIR pass, per monomorphized instance.
Before it, never-addressed locals become values and a storage pass picks
where each remaining place lives. The pass writes every retain, release,
uniqueness check and loan as a TIR statement with a reason. A checked
build verifies each reason; the emitter lowers the statements one to one
and decides nothing.

```text
fn bind(s: string) -> usize:
    t := s
    let u = t
    u.len()
```

```ir
today (release `own` and `counts` views of features/binding):
  %0 = Read move [$0]; Store [$1] %0; %2 = Read move [$1]; Store [$2] %2 ...
  exit sub 0 drops [$0]        summary kept [$0]      counts: release string

proposed (sketch):
  fn bind(s: string lent) -> usize
    %1 = Call bytes_len (%s lend)      # t and u are the value %s
    Return %1                           # no count statement: none has a reason
```

**The decision asked.** Approve this design, and with it reopen the fixed
constraint "work is done once per generic body where possible, not per
instance" ([Decision This Reopens](#decision-this-reopens)). Per-instance
work measured 1.03 times today's instructions through the passes.

## Illustrative Problems

Each group of #1364's plan is one missing fact. Every snippet is a
feature snapshot case on main dae8d82c7 (release, V8); "today" quotes its
`counts` or `wat` baseline. Snippets were parsed (see the
[Parse Log](#parse-log)).

### A. Releases of nothing

```text
use std.collections.{UniqList}

fn save(s: string) -> Result[void, string]:
    if s.len() > 0:
        return .Ok(())
    .Err(s + "?")

fn report(s: string) -> string:
    match save(s):
        .Ok(()) => "saved"
        .Err(message) => message

fn uniq_write(xs: &mut UniqList[string], i: usize, s: string) -> void:
    match xs.get_mut(i):
        .Some(r) => *r = s
        .None => panic("index")
```

Today: `report` releases `Result[(), string]` in the `.Ok` arm (#1381);
`uniq_write` releases a `(&mut string)?` that counts nothing (#1360).
`place_receiver` in `functions-receivers` releases an empty frame slot
(#1386). **Cause:** a `Drop` is placed by type before liveness, and also
ends a loan. **Design:** a release needs the reason "owned, last use, not
consumed". A value whose type counts nothing in this instance is never
owned. A matched variant with no counted payload is not owned past the
test. An empty place has no owned value. Loans end at their own
`LoanEnd`, so no release is kept for them.

### B. Copy-on-write checks in the wrong place

```text
use std.text.StringBuilder

fn keep_each(xs: List[string]) -> List[string]:
    let mut out: List[string] = []
    for x in xs:
        out.push(x)
    out

fn build(parts: &List[string]) -> string:
    sb := StringBuilder::new()
    for p in parts:
        sb.push(p)
        sb.push(",")
    sb.build()
```

Today: `keep_each` calls `rt:list_unique'1` on `out` once per element
(`for-loops.wat`, #1371). `build` copies `parts` with `rt:coll_copy`
because the body calls a writer (#1397). `through_ref` checks each element
of a `&List` (#1380); `rewarded` checks `out` three times (#1396).
**Cause:** no uniqueness fact, and "a call writes every place".
**Design:** a buffer is unique from its allocation until a retain of it,
a consuming use, or a call that may keep it. An `IsUnique` needs the
reason "may be shared", which its facts must support. `out` is fresh and
never retained, so its pushes carry no check. A loop keeps a snapshot
only if the loop's list is written while the loop runs
([Snapshot Iteration](../../spec/lang/06-control-flow.md#snapshot-iteration));
`sb.push` writes `sb`, not `parts`.

### C. Values kept or copied when only read

```text
fn over_value(xs: List[string]) -> usize:
    let n: usize = 0
    for x in xs:
        n += x.len()
    n

fn capture_call(s: string) -> usize:
    f := fn() -> usize: s.len()
    f() + f()
```

Today: `bind` (above) releases `s` and its caller retains it (#1365);
`over_value` retains and releases each element (#1366); `capture_call`
retains `s` into the environment and releases it at the exit (#1367);
`all!` in `requirements-join-list` copies `keys` on entry (#1392).
**Cause:** names are places; insertion copies, then patterns weaken.
**Design:** `s`, `t` and `u` are one value, lent by the caller. Each use
of it lends, so no count has a reason. `x` is a borrowed element read
whose one use lends. A capture at the value's last use consumes it, and
a lent source consumed takes exactly one retain.

### D. Heap environments and cells that never escape

```text
use std.ops.Disposable

struct Res:
    name: string
    log: *mut List[string]

impl Disposable for Res:
    fn dispose(&mut self) -> void:
        self.log.push(self.name)

fn two(log: *mut List[string], a: string, b: string) -> usize:
    using first = Res { name: a, log: log }
    using second = Res { name: b, log: log }
    first.name.len() + second.name.len()

fn apply(f: fn() -> usize) -> usize:
    f()

fn local(xs: &List[string]) -> usize:
    apply(fn() -> usize: xs.len())
```

Today: `capture_call` allocates an environment and, per call of `f`, a
cell to lend `s` to `string.len` (#1383, #1370). `two` calls
`rt:new/class-2` four times (#1382). `local#1` copies the captured list on
every call (#1395). Calling a held closure counts it (#1387). **Cause:**
storage is chosen in the emitter and `frame.rs`, where a `Fn` value is
always an escape. **Design:** the storage pass runs on the instance after
inlining. A closure only called in its body keeps its environment in
values. A cell no escaping closure shares is a frame slot or a value. A
`using` binding is a value. A captured `&List` is read in place, since a
local reference keeps nothing alive
([Local References](../../spec/lang/13-values-and-references.md#local-references)).

### E. Suspension frames and providers

```text
trait Tag:
    fn name(self) -> string

struct Named:
    label: string

impl Tag for Named:
    fn name(self) -> string:
        self.label

fn read_tag() -> usize $ Tag:
    $.use(Tag).name().len()

fn by_value(s: string) -> usize:
    $.with(Tag=Named { label: s }):
        read_tag()
```

Today: `by_value` retains the new `Named` and at once releases a
reference to it (#1390). `all!` saves `keys` and `t1` to `t68` at its one
`join!` (#1393). `block_on(fetch(key))` allocates a cold frame for a
known function (#1391). **Cause:** frames are laid out in the emitter
without liveness, and the provider conversion is emitter glue. **Design:**
the frame holds the values live across each point. A fresh provider moves
into its scope: one owner, no reason for a retain. A known callee under
`block_on` is a direct call after per-instance rechoice. Left: #1394,
row-polymorphic calls still build a context, because rows share one
instance ([Alternatives](#appendix-c-alternatives-considered), Option 3).

## The Design

### Pipeline

```ir
check ─► decode ─► per instance:
           substitute types, rechoose impls (instance_body, today)
           inline (release)                     hd_opt::inline
           values: never-addressed locals → SSA values
           storage: value | frame slot | heap cell, per place, env, frame
           ownership: one derivation → Retain/Release/IsUnique/Loan* with reasons
           verify (checked builds and CI)
         ─► emit: lowers each statement one to one ─► Wasm cleanup (no count rules)
```

Each stage is a TIR-to-TIR pass in `hd_tir`, as today's passes are. There
is no second IR. The per-instance path exists: `instance_body` already
substitutes, rechooses impls, inlines and runs the passes for one
instance, but only when a declaration-fact query needs it.

**Values.** A local is a value when no `Ref` of it survives inlining, no
closure shares it through a cell, and no cleanup suite reads it. Its reads
are the stored value; stores more than once get join values by Braun et
al.'s construction on structured control flow [S7]. Measured over the
bodies 25 programs reach (release, after inlining): 98.1% of 14,902
locals qualify, 88.1% have one definition, 10.0% need joins, 1.9% stay
places.

**Storage.** For each place local, closure environment, `using` binding,
default argument and suspension frame: a Wasm value when nothing takes
its address after inlining; a frame slot when its addresses do not
outlive the function; a heap cell otherwise. Whether an address outlives
the function comes from def-use on values: a value derived from a `Ref`
extends the place's life to its own last use, as Swift's
`mark_dependence` records it [S1]. The checker has already proven that
such references stay in scope, so no new rule is needed. This replaces
`frame.rs`'s per-local bit sets and their 64-candidate cap.

### The One Place Each Count Is Decided

The ownership pass decides from four facts per value: its source is
owned or lent; each use consumes or lends; its last use on each path;
whether its source may change while it lives. It writes a statement only
when a reason holds:

| Statement | Reason | It holds when |
| --- | --- | --- |
| `Retain v` | stored | a use consumes `v` (a store, a return, an owned argument, a capture) and `v` is lent or used again later |
| `Retain v` | second owner | `v` is read from a place that a write may change while `v` lives (including through a call or a `*T` alias) |
| `Release v` | last use | `v` is owned and its last use on this path lends, or it has none |
| `Release p` | overwritten | a store replaces an owned value in a place not known to be empty |
| `Release v` | cancelled | a suspension point is cancelled while `v` is live across it |
| `IsUnique b` | may be shared | `b` is not fresh, and since its last check a retain, a consuming use or a keeping call may have shared it |
| `LoanBegin`, `LoanEnd` | pointer loan | a reference through a `*T` outlives its expression, and elision found a possible conflicting access |
| any | view | `v` may be a representation view (a reference to a payload field its enum's slots scatter) |
| any | by design | the `&dyn` data count (#1354), a suspension's counted reference (#1355) |
| any | conservative | a fact was not computed (a size cap); reported, and its budget may only fall |

Read modes follow: a read is `move` at the place's last access, `borrow`
when the value only lends and its source does not change while it lives,
and `copy` only with a `Retain` beside it. This is Perceus's rule on
owned and borrowed values [S3], with Swift's +0 convention for
parameters [S2].

**Parameters.** A value parameter is lent in every build, as Swift's
normal parameters are; the callee retains what it consumes. In release,
the existing per-SCC summaries mark a parameter owned when the callee
consumes it on every path, as Lean's borrow inference does in reverse
[S4]. Interprocedural facts stay release-only. The cost: in dev, a callee
that stores a parameter takes one retain where today's by-type
convention moved it. Dev is not a performance target.

### Proving No Redundant Count Remains

1. **One producer.** Only the ownership pass writes count statements.
   The emitter lowers each to one helper call or inline sequence. A
   checked build asserts every emitted retain, release and uniqueness
   call traces to one TIR statement.
2. **A reason is a proof obligation.** The verifier recomputes the four
   facts by a slow, uncapped method and checks each statement's reason.
   A `Retain` whose value is never consumed, a `Release` before a later
   use, an `IsUnique` on a fresh buffer: each fails the checked build.
3. **Minimal relative to named facts.** Every remaining count traces to
   one fact. The facts are imprecise in three known places: a call not
   inlined and not summarized (dev) may write or keep its arguments; a
   write through a `*T` may reach any object of its type; a reference
   to a scattered payload field may be a view (#1422). The `own` view prints the fact behind
   each count, not the pattern that missed.
4. **Two backstops.** A CI report lists every count by reason over the
   bench programs and std, and "conservative" has a budget that may only
   fall. #1361's runtime detector keeps running on the bench.

**Against #1361 and #1362.** #1362 tags each operation with its reason
in today's layers. Each layer labels its own output, and nothing checks
the label; the glue's "conservative" cannot point to a fact. Here the
tag is the statement's reason field, and the verifier checks it. #1361 is
a runtime detector: it flags a retain and release pair that nothing
observed, but only on paths the corpus runs. It stays as a dynamic check;
with this design it should flag only sites whose reason is one of the
three imprecise facts. Step 1 below builds #1362's tags on today's layers
for a baseline; the verifier makes them checked.

### With Ownership And Lifetimes

- **Model A loans** ([Conflicts Through Aliases](../../spec/lang/13-values-and-references.md#conflicts-through-aliases),
  #1336). A loan is live until the last use of every holder. The pass
  places `LoanBegin` and `LoanEnd` at that range; a `Drop` no longer ends
  it. Elision keeps model A's per-reference rule over release write
  summaries, moved from `emit/elide.rs` into a TIR pass. Same-root
  conflicts stay compile errors in the checker.
- **Views.** A representation view is the one reference that owns an
  object (`crates/hd_wasm_linear/DESIGN.md` §23.20). Being a view becomes a
  per-value fact: only references to a payload field its enum's slots
  scatter may be views (#1421 removed erased calls; #1422 removes the rest). Only those take a release with the reason
  "view". The module-wide `views_absent` and the link fold go.
- **`Drop`, `Disposable` and `using`.** `std.rt.Drop` runs inside release
  glue ([Runtime Drop Hook](../../spec/lang/10-modules.md#runtime-drop-hook));
  the design moves no release later or earlier than today's last-use rule
  (DESIGN.md §23.20). `dispose` is a call in a cleanup suite
  ([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)),
  not a count, and keeps its lexical place. A cleanup suite's reads keep
  their locals as places.
- **Closures and `fn!`.** Captured immutable bindings are copied and
  variables are shared through a cell
  ([Copied And Shared Captures](../../spec/lang/07-functions.md#copied-and-shared-captures)).
  Where the cell lives is unobservable, so a cell no escaping closure
  shares is a frame slot or a value. A suspending closure's environment
  lives as long as the frame that holds it, so it escapes when the frame
  does.
- **Suspension frames**
  ([Suspending Functions](../../spec/lang/11-requirements-and-suspension.md#suspending-functions)).
  A frame holds the live values at each point. By-value arguments move
  in. A suspension that retains a counted reference its call lends
  ([Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results))
  keeps that retain, with a reason.
- **`List`/`Map` against `UniqList`/`UniqMap`**
  ([Collection Values](../../spec/lang/04-type-system.md#collection-values),
  [UniqList](../../spec/std/collections.md#uniqlist)). Copy-on-write
  buffers get `IsUnique` only with "may be shared". A `UniqList` is not
  `Copy`: it is moved, never retained, and never checked. The verifier
  rejects any `Retain` of a non-`Copy` value, as checked builds already
  trap on a retained buffer (b8382fa46).
- **`&dyn` and `&mut dyn`**
  ([Trait Values And `Any`](../../spec/lang/04-type-system.md#trait-values-and-any)).
  A trait reference lends like any reference. Its data count stays, with
  the reason "by design", until #1354 lands.
- **Tier 2 lifetimes.** The lifetime solver proves where each local
  reference points and that it stays in scope; a local reference keeps
  nothing alive ([Local References](../../spec/lang/13-values-and-references.md#local-references)).
  The design relies on that proof and adds no check. It needs only the
  liveness extension above, which def-use on values gives.

### Cost

| Component | Estimate (bottom-up, [Appendix D](#appendix-d-measurements)) |
| --- | --- |
| Rust code added / deleted | 5,080-8,900 / 7,650-12,230 lines, est. |
| web compiler gzip at the end | about −43 KB at midpoints; −120 KB to +31 KB at the extremes, est. (11-14 B per line, measured) |
| json release edit build | 160 ms measured today; about −1 ms at midpoints, −12% to +4% range, est. |
| peak memory | 72-121 MB measured today; within +1 MB, est. |
| output | smaller by every count, check and cell removed; not measured |

## Migration Plan

**The plan of record is #1399's checklist, not the table below.** Each
step there names its evidence: patterns over the blessed baselines of the
feature snapshot cases (`crates/hd_driver/tests/cases/features/`, the
feature map). Its feature cases are derived from that evidence.
`scripts/plancheck.py` checks the plan against the map. Every count
operation in the feature baselines must be matched by a step naming its
reason, every named code path must exist, and no feature case may
disappear (`crates/hd_driver/tests/feature-cases.lock`). The map is
additive: cases are added freely, and deleting or merging one needs the
owner. The table below is the original estimate, kept for its size and
time budgets.

Each step is about one hour of agent work, ships alone, and passes
default conformance and the stage snapshots. A snapshot diff may only
remove operations, except step 13's dev parameter retains. Each step
reports gzip, the json release edit build and peak memory, measured, next
to its estimate. **Stop rule:** a step whose measured gzip, time or
memory exceeds 1.5 times its upper estimate stops and reports before
merging, and so does any step that would leave under 5 KB of headroom.

Status (2026-11-07): the old-pass names in this table are estimates from the design. Measured since: `known.rs` and `late.rs` stay, as every rule still changes output (#1399 12f). Drop flags as TIR locals (step 24) came out larger, so that patch isn't merged. Step 17's storage marks would move about 700 lines rather than remove them, and that's an open question for the owner.

Estimates: gzip at 11-14 B per Rust line; time in ms on the json release
edit build (160 ms measured); memory as peak RSS. "−" deletes nothing.

| # | Step | Deletes (old layer) | gzip, est. | ms, est. | MB, est. |
| --- | --- | --- | ---: | ---: | ---: |
| 1 | reason field on count statements and emitter glue; `counts` prints it (#1362 baseline) | − | +0.9 to +2.1 KB | 0 | 0 |
| 2 | every reached instance goes through `instance_body` in release | the `candidate` filter | −0.1 to +1.9 | 0 to +3.4 | 0 to +0.6 |
| 3 | per-instance ownership in dev, without the inliner | − | +0.6 to +1.7 | dev +0.5 to +1.9 | 0 |
| 4 | `counted` answers yes or no | `Tri::Maybe` paths; emitter's scalar-instance fix-up | −4.2 to −1.2 | 0 | 0 |
| 5 | classify value locals; `own` prints them | − | +1.1 to +2.8 | +0.2 to +0.5 | 0 |
| 6 | single-definition value locals forward their value | `own/late.rs` | −1.3 to +2.9 | +0.3 to +1 | 0 |
| 7 | join values for locals stored more than once | − | +3.3 to +6.3 | +1 to +2 | +0.1 to +0.3 |
| 8 | uses, consume or lend, and last uses per value | − | +3.3 to +6.3 | +1 to +3 | 0 |
| 9 | read modes of value locals from step 8 | value rules of `borrow.rs`, `moves.rs` | −9.0 to −0.6 | −1 to +0.5 | 0 |
| 10 | releases of value locals from step 8 | value rules of `pairs.rs`, `known.rs` | −9.0 to −0.6 | −1 to +0.5 | 0 |
| 11 | read modes of place locals; dependence extends liveness | rest of `borrow.rs` | −7.0 to +1.2 | −1 to +0.5 | 0 |
| 12 | releases of place locals | `insert.rs`, rest of `moves.rs`, `pairs.rs`, `known.rs` | −23.0 to −8.3 | −5 to −1 | 0 |
| 13 | value parameters lent in every build | emitter `push_args` and entry retains | −5.9 to −0.5 | 0 | 0 |
| 14 | release: owned parameters where every path consumes | lend half of the release summaries | −10.1 to −1.6 | −0.5 to 0 | 0 |
| 15 | uniqueness facts in straight-line code (#1371, #1396) | part of `hoist.rs` | −0.6 to +3.1 | +0.2 to +0.5 | 0 |
| 16 | uniqueness across loops (#1380, #1397) | rest of `hoist.rs` | −7.3 to −1.6 | −0.5 to 0 | 0 |
| 17 | storage marks in TIR, written from today's choices | cell choice in `emit/closure.rs` | −2.5 to +1.9 | 0 | 0 |
| 18 | closure environments from escape on the instance; known `CallValue` inlined (#1383, #1385, #1387) | − | +0.7 to +5.0 | 0 to +1 | 0 |
| 19 | cells, `using` and `&mut []` defaults in slots or values (#1370, #1382, #1395) | `own/frame.rs` candidate tables | −7.6 to −0.6 | −0.5 to +0.5 | 0 |
| 20 | `LoanBegin`/`LoanEnd` statements (#1360) | loan decisions in `emit/loan.rs`, `emit/borrow.rs` | −8.4 to +0.1 | 0 to +0.5 | 0 |
| 21 | loan elision as a TIR pass | `emit/elide.rs` | −5.5 to +0.5 | −0.5 to +0.5 | 0 |
| 22 | views as a per-value fact | `views_absent`; the link fold | −4.5 to +0.6 | 0 | 0 |
| 23 | `Retain`/`Release` statements; reads imply no count | `release_untaken`, exit-set glue in `emit/count.rs` | −12.1 to −0.7 | −3 to −1 | 0 |
| 24 | drop flags as TIR locals; assert every emitted count has a statement | `emit/drop_flags.rs`, `emit/promote.rs` | −5.1 to −0.9 | −1 to 0 | 0 |
| 25 | delete the Wasm count rules once the corpus shows no hit | `static_values`, `static_counts`, `count_pairs` | −5.6 to −4.4 | −2.6 to −1.5 | 0 |
| 26 | live sets at suspension points (#1393) | save-all in `emit/susp.rs` | −2.0 to +4.5 | −1 to +0.5 | 0 |
| 27 | suspending calls and providers take moves (#1392, #1390); known `block_on` drives at once (#1391) | cold-constructor retains | −3.0 to +1.0 | 0 | 0 |
| 28 | `&mut` callees inline per instance (#1205) | − | +1.1 to +3.5 | 0 to +1 | 0 |
| 29 | if frames remain after 28: small `&mut` structs cross a call as values | − | +3.3 to +7.0 | 0 to +0.5 | 0 |
| 30 | reason verifier, behind a feature the web compiler leaves off (as `analysis`) | − | 0 | checked builds only | 0 |
| 31 | CI corpus report by reason; the "conservative" budget | − | 0 | − | 0 |

Running gzip total, est.: at midpoints the peak is +12.7 KB after step 8,
and the end is −43 KB. With every add at its top and every delete at its
bottom, it peaks at +24.4 KB after step 28, and +31.4 KB if step 29 runs.
Headroom today is 33,291 B, so step 29 runs only if the measured total
leaves room. Steps 2, 9, 12, 13 and 23 each report the default-tier V8
bench table; step 2 is the first visible result: `hash_of[usize]`'s bound
call should become an impl call (expected, not measured).

In-flight work: #1360's option C finishes first; the audit
tickets become the acceptance cases of the steps that name them.

## Decision This Reopens

**Work once per generic body, not per instance.** It is a fixed constraint
in [TIR v2 directions](../compiler/tir-v2-directions.md#fixed-constraints)
("Work is done once per generic body where possible, not per instance"),
and TIR v2 builds on it ("Why once per body, not per instance";
"generic bodies keep symbolic places", whose count half this ends). This
design needs the owner to lift it for inlining and ownership.

The measured cost: the instructions a per-instance run sees are 1.03
times those the per-body run sees in release, 1.04 in dev, on 25
programs; 1.17 at most (json, dev). The gain: no `Maybe`, so no count is
kept "in case"; bound calls resolve to impl calls before inlining, which
is one of #1205's two causes. The other constraints hold: one IR,
interprocedural work release-only, no whole-program analysis beyond
model A's existing release summaries.

## Appendix A. Stress Test

| Case | What could break | What the design does |
| --- | --- | --- |
| `let mut out = values; for value in out: out.push(value)` (spec's snapshot example) | lending `out` to the loop would let the push change what the loop walks | the loop's read has a write to its source in range: "second owner" retain; the push copies once, as the spec says |
| `xs := p.items; p.items.push("x"); xs.len()` with `p: *mut Rec` | `xs` lent from a place a `*T` alias writes | a write through any `*mut Rec` may change the source: "second owner" retain |
| `bind_kept(xs: List[string], all: &mut List[List[string]])` pushes `ys := xs` | lent parameter consumed | dev: one "stored" retain (a cost, above); release: the summary makes `xs` owned, no count, as today |
| closure stored in a struct field after one local call | storage chosen before the store is seen | storage runs after inlining on the whole instance; any store is an escape: heap environment |
| a captured `let mut` shared by a closure that a suspension frame holds | a frame slot dies with the function | the frame escapes, so its closures escape, so the cell is a heap cell |
| a `&dyn Codec` generic method returning a payload reference | a view released as a plain reference leaks | erased results are "maybe view": release with the view reason, guarded by `rt:ref_is_view` |
| a polymorphic recursive generic | per-instance work explodes | passes run only on instances the emitter already emits; no new instance is made |
| a suspension cancelled between two points | values live across it leak | the live set at that point is the unwind set: "cancelled" releases |
| `Weak[T]` upgrade after the last strong use | an earlier release changes what `upgrade` sees | releases stay at today's last use (§23.20); none moves earlier |
| a body over 64 addressed locals | `frame.rs` gives up past 64 candidates | dependence on values has no cap; a fact cut short is "conservative" and counted |

Two cases cost something: dev's lent parameters add a retain where a
callee stores a parameter, and #1394's row contexts stay.

## Appendix B. Prior Art

**Swift SIL, Ownership SSA** [S1]. Every value is owned or guaranteed;
copies and destroys are instructions, and the verifier checks lifetimes.

```swift
// SIL, simplified from the Ownership doc's examples
// sil [ossa] @bind : $@convention(thin) (@guaranteed String) -> Int
bb0(%0 : @guaranteed $String):
  %1 = function_ref @len : $@convention(method) (@guaranteed String) -> Int
  %2 = apply %1(%0)          // no copy_value: %0 is borrowed for the call
  return %2
```

hd takes the per-function ownership invariant, +0 parameters [S2],
`mark_dependence` for liveness, and Swift's migration by function mode.
It rejects a second IR (the owner's "one IR").

**Koka, Perceus** [S3]. `dup` and `drop` derive from ownership and last
use in one pass; the result is precise for its inputs.

```koka
// Perceus output, simplified from the paper's map example
fun map(xs, f)
  match xs
    Cons(x, xx) -> { dup(x); dup(xx); drop(xs); Cons(f(x), map(xx, f)) }
    Nil -> Nil
```

hd takes the derivation. It rejects uniform boxing: hd specializes.

**Lean 4, λRC** [S4]. `inc` and `dec` go into an IR that has none;
parameters are inferred borrowed (`@&`) when the body only reads them.

```lean
def len (s : @& String) : Nat := s.length   -- borrowed: no inc, no dec
```

hd takes borrowed inference as the release refinement of lent-by-default.

**Rust MIR, drop elaboration** [S5]. Drops are placed per path, with
drop flags where a value is moved on some paths only.

```rust
let s = make();          // MIR: _1 = make()
if c { consume(s); }     //       drop flag set false on this path
                         // drop(_1) only if the flag is still set
```

hd takes drop flags as TIR locals (step 24). Rust runs MIR per generic
body and leaves instances to LLVM [S6]; hd has no LLVM after it, which is
why it decides per instance.

## Appendix C. Alternatives Considered

- **Option 2, a low ownership IR after monomorphization.** It may end
  smaller, but needs a parallel backend: +234 to +430 KB of gzip while
  both live (est.), seven to thirteen times the headroom. It also reopens
  "one IR".
- **Option 3, close the world** (no erased generics on `dyn`, per-row
  instances, lambda sets). It removes views and #1394 but leaves groups B
  and C, and 3a removes language rules. It can follow this design.
- **Option 4, borrow by meaning.** It fixes only group C, and every
  snapshot of a changing place would need an explicit copy.
- **Option 5, tracing with a shared bit.** It breaks "RC on linear
  memory" and leaves group B and #1205.
- **Keep the plan A-E as point fixes.** Each fix is a new pattern in one
  of six layers, and nothing shows when the last one is found.

## Appendix D. Measurements

Measured at 4336341bd: release `hd` on an Apple M3 Max shared with other
jobs, `HD_JOBS=1`, fresh `HD_CACHE`, best of three, 25 programs
(`bench/study`, `bench/runtime/progs`, `examples/dogfood`). Stage times
come from `Counters::stage_ns`; times inside a stage from scratch timers,
not committed.

| Fact | Value |
| --- | --- |
| web compiler gzip | 1,966,709 B; cap 2,000,000 B (`scripts/test-web-compiler.js`) |
| gzip per Rust code line | 11.2 B (passes stubbed: −65,106 B), 15 B (Wasm count rules: −6,070 B), 12 B (inliner: −9,882 B) |
| code lines today | passes 8,002; verifier 1,498; driver `own.rs` 1,468; inliner 1,051; emitter 17,536, of which ownership glue 5,102; `opt.rs` 2,682 |
| json release edit build | 160 ms; passes 16.6; inliner 3.6; emission 28.6; Wasm cleanup 41.3, of which count rules 2.6 |
| per-instance ÷ per-body instructions | 1.03 release, 1.04 dev; at most 1.17 |
| pass rate | 0.95 µs per TIR instruction (0.42 to 0.97 across programs) |
| peak RSS | 107-121 MB cold, 72-84 MB edit |
| locals after inlining (release) | 14,902; 1.9% addressed; 0.01% shared cells; 88.1% defined once |
| count lines in the 115 feature `counts` baselines | 882: 607 release, 261 retain, 14 copy (main dae8d82c7) |

The per-step estimates in the migration table rest on these: lines per
step from the files each replaces, at the per-line rate above; time from
the pass and emission rates.

## Appendix E. Details

- **Pipeline position.** After decode and substitution, before emission,
  per instance. Since step 2, each release instance's own summary is its
  calling convention, and the passes run once per instance.
- **Invariants.** Every count statement has a reason; every `Retain` is of
  a `Copy` value; no emitted count without a statement; read modes imply
  no count after step 23.
- **Complexity.** Value construction is linear in instructions with
  joins at block exits [S7]; liveness is one backward walk per body; no
  cap on locals.
- **Caching and determinism.** Instance bodies are not cached across
  runs, as emission output is not; the wire format of checked TIR is
  unchanged.
- **Tests.** Each step's snapshot diffs; the verifier in the checked
  conformance mode; the corpus report in CI.
- **Rollback.** Per step: each deletes only what it replaced.

## Sources

| # | Source |
| --- | --- |
| S1 | Swift, SIL Ownership: https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md |
| S2 | Swift, proposal to change the default ownership for passing parameters (+1 to +0, 2018): https://forums.swift.org/t/proposal-to-change-the-default-ownership-for-passing-parameters/10261 |
| S3 | Reinking, Xie, de Moura and Leijen, Perceus (PLDI 2021): https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/ |
| S4 | Ullrich and de Moura, Counting Immutable Beans (IFL 2019): https://arxiv.org/abs/1908.05647 |
| S5 | rustc dev guide, drop elaboration: https://rustc-dev-guide.rust-lang.org/mir/drop-elaboration.html |
| S6 | rust-lang/rust #91743, enable the MIR inliner: https://github.com/rust-lang/rust/pull/91743 |
| S7 | Braun et al., Simple and Efficient Construction of SSA Form (CC 2013): https://doi.org/10.1007/978-3-642-37051-9_6 |

## Parse Log

Each `text` block was parsed with `scripts/hd-conformance.sh parse` on
this branch (base main dae8d82c7). Parsing checks syntax only; no block
is claimed to type-check. `ir` blocks are sketches and are not parsed.

| Block | Content | Result |
| --- | --- | --- |
| 1 | `bind` | parse |
| 2 | group A: `save`, `report`, `uniq_write` | parse |
| 3 | group B: `keep_each`, `build` | parse |
| 4 | group C: `over_value`, `capture_call` | parse |
| 5 | group D: `Res`, `two`, `apply`, `local` | parse |
| 6 | group E: `Tag`, `Named`, `read_tag`, `by_value` | parse |
